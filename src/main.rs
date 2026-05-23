#![allow(unused_imports)]
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

fn main() -> Result<(), anyhow::Error> {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    // let msg = "Logs from your program will appear here!";
    // println!("{msg}");

    // Uncomment the code below to pass the first stage

    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                println!("accepted new connection");
                handle_connection(stream)?;
            }
            Err(e) => {
                eprintln!("error: {}", e);
            }
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream) -> Result<(), anyhow::Error> {
    loop {
        let mut buf = [0;12];
        if let Err(err) = stream.read(&mut buf) {
            anyhow::bail!("Error while reading message {err}")
        }
        println!("Got message {buf:?}");
        if let Err(err) = stream.write(b"+PONG\r\n") {
            anyhow::bail!("Error while writing back response {err}")
        }
    }
}
