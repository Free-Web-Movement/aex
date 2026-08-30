//! Integration tests for the proxy-protocol detector examples.
//!
//! These exercise the development material in `examples/detectors_common`
//! through real sockets: sniffing dispatch, TLS termination, trojan/VLESS
//! header validation, SOCKS detection.

#[path = "../examples/detectors_common/mod.rs"]
mod detectors_common;

use std::net::SocketAddr;
use std::sync::Arc;

use detectors_common::{
    parse_client_hello, ClientHelloParse, SocksDetector, SocksVersion,
    TlsClientHello as _, TlsDetector, TlsLoader, TlsMiddleware, TrojanMiddleware,
    TrojanRequestInfo, VlessMiddleware, VlessRequestInfo,
};
use aex::unified::detect::ProtocolDetector;
use detectors_common::TlsSession;
use rcgen::generate_simple_self_signed;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls::{
    self,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
    DigitallySignedStruct, SignatureScheme,
};
use tokio_rustls::TlsConnector;

use aex::connection::context::{BoxReader, Context};
use aex::connection::global::GlobalContext;
use aex::unified::UnifiedServer;

// ---------------------------------------------------------------------------
// Test scaffolding
// ---------------------------------------------------------------------------

fn free_addr() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

fn spawn_server(addr: SocketAddr, configure: impl FnOnce(UnifiedServer) -> UnifiedServer) {
    let globals = Arc::new(GlobalContext::new(addr, None));
    let server = configure(UnifiedServer::new(addr, globals));
    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("server exited: {e}");
        }
    });
}

async fn wait_listening(addr: SocketAddr) {
    for _ in 0..100 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("server never became reachable at {addr}");
}

fn test_certs() -> (CertificateDer<'static>, Vec<u8>) {
    let ck = generate_simple_self_signed(vec!["example.com".into()]).unwrap();
    let cert_der = ck.cert.der().to_vec();
    let key_der = ck.key_pair.serialize_der();
    (CertificateDer::from(cert_der), key_der)
}

/// Build a PrivateKeyDer from raw PKCS#8 DER bytes.
fn der_key(mut key_der: Vec<u8>) -> rustls::pki_types::PrivateKeyDer<'static> {
    rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        std::mem::take(&mut key_der),
    ))
}

/// Accept-any certificate verifier for connecting to our own self-signed
/// test server.
#[derive(Debug)]
struct NoVerify;

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

fn insecure_client_config() -> rustls::ClientConfig {
    rustls::ClientConfig::builder_with_provider(Arc::new(ring_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth()
}

fn ring_provider() -> rustls::crypto::CryptoProvider {
    rustls::crypto::ring::default_provider()
}

// ---------------------------------------------------------------------------
// Parser unit tests
// ---------------------------------------------------------------------------

/// Hand-craft a minimal ClientHello carrying SNI + ALPN.
fn craft_client_hello(sni: Option<&str>, alpn: &[&str]) -> Vec<u8> {
    let mut exts: Vec<u8> = Vec::new();

    // SNI extension (type 0x0000): data = u16 list_len + entries.
    if let Some(name) = sni {
        let mut entry = vec![0u8]; // name_type = host_name
        entry.extend_from_slice(&(name.len() as u16).to_be_bytes());
        entry.extend_from_slice(name.as_bytes());
        let mut data = (entry.len() as u16).to_be_bytes().to_vec();
        data.extend_from_slice(&entry);
        let mut ext = 0x0000u16.to_be_bytes().to_vec();
        ext.extend_from_slice(&(data.len() as u16).to_be_bytes());
        ext.extend_from_slice(&data);
        exts.extend_from_slice(&ext);
    }

    // ALPN extension (type 0x0010).
    if !alpn.is_empty() {
        let mut list: Vec<u8> = Vec::new();
        for proto in alpn {
            list.push(proto.len() as u8);
            list.extend_from_slice(proto.as_bytes());
        }
        let mut ext = 0x0010u16.to_be_bytes().to_vec();
        ext.extend_from_slice(&((list.len() + 2) as u16).to_be_bytes());
        ext.extend_from_slice(&(list.len() as u16).to_be_bytes());
        ext.extend_from_slice(&list);
        exts.extend_from_slice(&ext);
    }

    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(&[0x03, 0x03]); // client_version TLS1.2
    body.extend_from_slice(&[0u8; 32]); // random
    body.push(0); // session_id len
    body.extend_from_slice(&2u16.to_be_bytes()); // cipher_suites len
    body.extend_from_slice(&[0x13, 0x01]); // one suite
    body.push(1); // compression methods len
    body.push(0);
    body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
    body.extend_from_slice(&exts);

    let mut hs: Vec<u8> = vec![0x01]; // handshake type: client_hello
    hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    hs.extend_from_slice(&body);

    let mut record: Vec<u8> = vec![0x16, 0x03, 0x01];
    record.extend_from_slice(&(hs.len() as u16).to_be_bytes());
    record.extend_from_slice(&hs);
    record
}

#[tokio::test]
async fn parser_extracts_sni_and_alpn() {
    let hello_bytes = craft_client_hello(Some("api.example.com"), &["h2", "http/1.1"]);
    match parse_client_hello(&hello_bytes) {
        ClientHelloParse::Complete(hello) => {
            assert_eq!(hello.sni.as_deref(), Some("api.example.com"));
            assert_eq!(hello.alpn, vec!["h2", "http/1.1"]);
        }
        other => panic!("expected complete parse, got {other:?}"),
    }

    // Fragmented delivery must eventually reach the same verdict.
    let mut state = aex::unified::detect::DetectionState::new();
    for cut in [5usize, 20, 60] {
        if cut >= hello_bytes.len() {
            break;
        }
        let verdict = TlsDetector.detect(&hello_bytes[..cut], &mut state);
        assert!(
            matches!(verdict, aex::unified::detect::Verdict::NeedMore(_)),
            "cut={cut} should still be pending"
        );
    }
    let verdict = TlsDetector.detect(&hello_bytes, &mut state);
    assert!(matches!(verdict, aex::unified::detect::Verdict::Match));

    // Non-TLS garbage is passed on immediately.
    assert!(matches!(
        parse_client_hello(b"GET / HTTP/1.1\r\n"),
        ClientHelloParse::NotTls
    ));
}

#[tokio::test]
async fn socks_detector_verdicts() {
    let mut state = aex::unified::detect::DetectionState::new();

    // Partial SOCKS5 greeting stays pending until all method bytes arrive.
    assert!(matches!(
        SocksDetector.detect(&[0x05, 0x02, 0x00], &mut state),
        aex::unified::detect::Verdict::NeedMore(_)
    ));
    assert!(matches!(
        SocksDetector.detect(&[0x05, 0x02, 0x00, 0x02], &mut state),
        aex::unified::detect::Verdict::Match
    ));

    let mut state = aex::unified::detect::DetectionState::new();
    SocksDetector.detect(&[0x05, 0x02, 0x00, 0x02], &mut state);
    assert_eq!(state.get_scratch::<SocksVersion>(), Some(SocksVersion::V5));

    // HTTP traffic passes through untouched.
    let mut state = aex::unified::detect::DetectionState::new();
    assert!(matches!(
        SocksDetector.detect(b"GET / HTTP/1.1\r\n\r\n", &mut state),
        aex::unified::detect::Verdict::Pass
    ));
}

// ---------------------------------------------------------------------------
// End-to-end: sniffing + TLS termination + validators over real sockets
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tls_sniff_and_termination_e2e() {
    let addr = free_addr();
    let (cert, key) = test_certs();
    let loader = TlsLoader::from_der(vec![cert], der_key(key)).unwrap();

    spawn_server(addr, |s| {
        s.detector(Arc::new(TlsDetector))
            .custom_handler(
                "tls",
                Arc::new(move |ctx: Context| {
                    let loader = loader.clone();
                    tokio::spawn(async move {
                        let mut ctx = ctx;
                        let accept = TlsMiddleware::accept(loader);
                        if !accept(&mut ctx).await {
                            return;
                        }
                        // Report negotiated names back to the test client.
                        let session = ctx.local.get_ref::<TlsSession>().cloned();
                        if let Some(w) = ctx.writer.as_mut() {
                            let msg = format!(
                                "sni={:?} alpn={:?} echo-ok\n",
                                session.as_ref().and_then(|s| s.sni.clone()),
                                session.as_ref().and_then(|s| s.alpn.clone())
                            );
                            let _ = w.write_all(msg.as_bytes()).await;
                        }
                    })
                }),
            )
            .tcp_handler(Arc::new(|ctx: Context| {
                tokio::spawn(async move {
                    let mut ctx = ctx;
                    if let Some(w) = ctx.writer.as_mut() {
                        let _ = w.write_all(b"plain\n").await;
                    }
                })
            }))
    });
    wait_listening(addr).await;

    // Plain connection must NOT be treated as TLS.
    let mut plain = TcpStream::connect(addr).await.unwrap();
    plain.write_all(b"anything").await.unwrap();
    let mut buf = vec![0u8; 64];
    let n = plain.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"plain\n");

    // TLS connection gets terminated and echoed in the clear.
    let connector = TlsConnector::from(Arc::new(insecure_client_config()));
    let tcp = TcpStream::connect(addr).await.unwrap();
    let mut tls = connector
        .connect(ServerName::try_from("example.com").unwrap(), tcp)
        .await
        .unwrap();
    tls.write_all(b"ping").await.unwrap();
    let mut buf = vec![0u8; 128];
    let n = tls.read(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf[..n]).to_string();
    assert!(text.contains("echo-ok"), "got: {text}");
    assert!(text.contains(r#"sni=Some("example.com")"#), "got: {text}");
}

#[tokio::test]
async fn trojan_over_tls_e2e() {
    let addr = free_addr();
    let (cert, key) = test_certs();
    let loader = TlsLoader::from_der(vec![cert], der_key(key)).unwrap();

    spawn_server(addr, |s| {
        s.detector(Arc::new(TlsDetector))
            .custom_handler(
                "tls",
                Arc::new(move |ctx: Context| {
                    let loader = loader.clone();
                    tokio::spawn(async move {
                        let mut ctx = ctx;
                        let tls = TlsMiddleware::accept(loader);
                        if !tls(&mut ctx).await {
                            return;
                        }
                        let trojan = TrojanMiddleware::validate();
                        if !trojan(&mut ctx).await {
                            return;
                        }
                        let req = ctx.local.get_ref::<TrojanRequestInfo>().cloned().unwrap();
                        if let Some(w) = ctx.writer.as_mut() {
                            let _ = w
                                .write_all(format!("{}:{}\n", req.target, req.port).as_bytes())
                                .await;
                        }
                        // Payload after the header should flow untouched.
                        let mut rest = Vec::new();
                        if let Some(r) = ctx.reader.as_mut() {
                            let _ = r.read_to_end(&mut rest).await;
                        }
                    })
                }),
            )
    });
    wait_listening(addr).await;

    let hash = "a".repeat(56);
    let mut request: Vec<u8> = hash.clone().into_bytes();
    request.extend_from_slice(b"\r\n");
    request.push(0x01); // ATYP IPv4
    request.extend_from_slice(&[1, 2, 3, 4]); // address
    request.extend_from_slice(&443u16.to_be_bytes()); // port
    request.extend_from_slice(b"\r\n"); // terminator

    let connector = TlsConnector::from(Arc::new(insecure_client_config()));
    let tcp = TcpStream::connect(addr).await.unwrap();
    let mut tls = connector
        .connect(ServerName::try_from("example.com").unwrap(), tcp)
        .await
        .unwrap();
    tls.write_all(&request).await.unwrap();
    let mut buf = vec![0u8; 64];
    let n = tls.read(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf[..n]).to_string();
    assert_eq!(text.trim_end(), "1.2.3.4:443");
}

#[tokio::test]
async fn vless_over_tls_e2e() {
    let addr = free_addr();
    let (cert, key) = test_certs();
    let loader = TlsLoader::from_der(vec![cert], der_key(key)).unwrap();

    spawn_server(addr, |s| {
        s.detector(Arc::new(TlsDetector))
            .custom_handler(
                "tls",
                Arc::new(move |ctx: Context| {
                    let loader = loader.clone();
                    tokio::spawn(async move {
                        let mut ctx = ctx;
                        let tls = TlsMiddleware::accept(loader);
                        if !tls(&mut ctx).await {
                            return;
                        }
                        let vless = VlessMiddleware::validate();
                        if !vless(&mut ctx).await {
                            return;
                        }
                        let req = ctx.local.get_ref::<VlessRequestInfo>().cloned().unwrap();
                        if let Some(w) = ctx.writer.as_mut() {
                            let _ = w
                                .write_all(format!("{}:{}\n", req.target, req.port).as_bytes())
                                .await;
                        }
                    })
                }),
            )
    });
    wait_listening(addr).await;

    // VLESS request: ver(0) + uuid16 + addon_len(0) + cmd(1=TCP) + port + ATYP
    let mut request: Vec<u8> = vec![0x00];
    request.extend_from_slice(&[0xAA; 16]); // uuid
    request.push(0); // addons len
    request.push(0x01); // command TCP
    request.extend_from_slice(&8443u16.to_be_bytes());
    request.push(0x03); // ATYP domain
    request.push(11);
    request.extend_from_slice(b"example.com");
    request.extend_from_slice(b"payload-after-header");

    let connector = TlsConnector::from(Arc::new(insecure_client_config()));
    let tcp = TcpStream::connect(addr).await.unwrap();
    let mut tls = connector
        .connect(ServerName::try_from("example.com").unwrap(), tcp)
        .await
        .unwrap();
    tls.write_all(&request).await.unwrap();
    let mut buf = vec![0u8; 64];
    let n = tls.read(&mut buf).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&buf[..n]).trim_end(), "example.com:8443");
}

#[tokio::test]
async fn socks5_sniff_dispatch_e2e() {
    let addr = free_addr();
    spawn_server(addr, |s| {
        s.detector(Arc::new(SocksDetector)).custom_handler(
            "socks",
            Arc::new(|ctx: Context| {
                tokio::spawn(async move {
                    let mut ctx = ctx;
                    let version = ctx
                        .local
                        .get_ref::<aex::unified::detect::DetectionState>()
                        .and_then(|s| s.get_scratch::<SocksVersion>());
                    if let Some(w) = ctx.writer.as_mut() {
                        let line = match version {
                            Some(SocksVersion::V5) => "v5",
                            Some(SocksVersion::V4) => "v4",
                            None => "?",
                        };
                        let _ = w.write_all(line.as_bytes()).await;
                    }
                })
            }),
        )
    });
    wait_listening(addr).await;

    let mut sock = TcpStream::connect(addr).await.unwrap();
    sock.write_all(&[0x05, 0x02, 0x00, 0x02]).await.unwrap();
    let mut buf = vec![0u8; 8];
    let n = sock.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"v5");
}

#[tokio::test]
async fn malformed_trojan_header_is_rejected() {
    use detectors_common::TrojanMiddleware;

    // Feed a broken header through the validator on an in-memory duplex.
    let (mut client_side, mut server_side) = tokio::io::duplex(1024);

    let task = tokio::spawn(async move {
        let reader: aex::connection::context::BoxReader =
            Box::new(tokio::io::BufReader::new(server_side));
        let writer = Box::new(tokio::io::sink());
        let globals = Arc::new(GlobalContext::new(free_addr(), None));
        let mut ctx =
            Context::new(Some(reader), Some(writer), globals, free_addr());
        TrojanMiddleware::validate()(&mut ctx).await
    });

    // Only 10 bytes of the required 59 arrive, then EOF.
    client_side
        .write_all(&b"deadbeef00"[..10])
        .await
        .unwrap();
    drop(client_side);

    assert_eq!(task.await.unwrap(), false);
}
