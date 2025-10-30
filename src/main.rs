#![allow(unused_imports)]
use std::net::TcpListener;

fn main() {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    println!("Logs from your program will appear here!");

    // Uncomment the code below to pass the first stage
    //
    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();

    loop {
        let stream = listener.accept();
        match stream {
            Ok((_, addr)) => {
                println!("accepted new connection from host {:?} and port {}", addr.ip(), addr.port());
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }
    }
}
