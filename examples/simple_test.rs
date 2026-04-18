use std::net::TcpListener;
use std::io::Write;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
    println!("Server on 127.0.0.1:8080");
    
    for stream in listener.incoming() {
        let mut stream = stream.unwrap();
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nHello";
        let _ = stream.write_all(response);
    }
}