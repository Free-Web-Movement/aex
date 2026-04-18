//! Benchmark test without AEX framework - to isolate bottleneck

use std::net::TcpListener;
use std::thread;

fn handle_client(mut stream: std::net::TcpStream) {
    use std::io::{Read, Write};
    
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                // Simulate what AEX does - parse HTTP request
                let request = std::str::from_utf8(&buf[..n]).unwrap_or("");
                
                if request.starts_with("GET / ") || request.starts_with("GET /\r") {
                    // Parse headers like AEX does
                    let mut lines = 0;
                    for line in request.lines() {
                        if line.is_empty() { break; }
                        lines += 1;
                    }
                    // Route matching (Trie simulation)
                    let path = "/";
                    
                    // Build response (format! like AEX does)
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHello");
                    if stream.write_all(response.as_bytes()).is_err() { break; }
                } else if request.starts_with("GET /api/users") {
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: 24\r\n\r\n[{{\"id\":1,\"name\":\"alice\"}}]");
                    if stream.write_all(response.as_bytes()).is_err() { break; }
                } else if request.starts_with("GET /api/users/") {
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n{{\"id\":1}}");
                    if stream.write_all(response.as_bytes()).is_err() { break; }
                } else {
                    let response = format!("HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found");
                    if stream.write_all(response.as_bytes()).is_err() { break; }
                }
                let _ = stream.flush();
            }
            Err(_) => break,
        }
    }
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    println!("AEX-simulator listening on 127.0.0.1:8080");
    
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            thread::spawn(|| handle_client(stream));
        }
    }
}