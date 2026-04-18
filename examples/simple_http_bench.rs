//! Simple HTTP Benchmark - blocking multi-threaded

use std::net::TcpListener;
use std::io::{Read, Write};
use std::thread;

fn handle_client(mut stream: std::net::TcpStream) {
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let request = String::from_utf8_lossy(&buf[..n]);
                let response = if request.starts_with("GET / ") || request.starts_with("GET /\r") {
                    "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHello"
                } else if request.starts_with("GET /json") {
                    "HTTP/1.1 200 OK\r\nContent-Length: 18\r\n\r\n{\"message\":\"hello\"}"
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHello"
                };
                if stream.write_all(response.as_bytes()).is_err() { break; }
                let _ = stream.flush();
            }
            Err(_) => break,
        }
    }
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    println!("Simple HTTP listening on 127.0.0.1:8080");
    
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            thread::spawn(|| handle_client(stream));
        }
    }
}