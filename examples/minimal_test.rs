//! Minimal AEX test - just read and write raw bytes

use std::net::TcpListener;
use std::io::{Read, Write};

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    println!("Minimal test on 127.0.0.1:8080");
    
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        let mut buf = [0u8; 1024];
        
        // Just read and check GET /
        let n = stream.read(&mut buf).unwrap();
        if n > 0 && buf.starts_with(b"GET /") {
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHello").unwrap();
        }
    }
}