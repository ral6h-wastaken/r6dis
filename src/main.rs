#![allow(unused_imports)]
use std::{io::Write, net::TcpListener};

fn main() -> Result<(), anyhow::Error> {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    // let msg = "Logs from your program will appear here!";
    // println!("{msg}");

    // Uncomment the code below to pass the first stage

    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                println!("accepted new connection");
                if let Err(err) = stream.write(b"+PONG\r\n") {
                    anyhow::bail!("Error while writing back response {}", err )
                }
            }
            Err(e) => {
                eprintln!("error: {}", e);
            }
        }
    }

    Ok(())
}
