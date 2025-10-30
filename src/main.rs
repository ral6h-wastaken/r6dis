#![allow(unused_imports)]
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;

fn main() {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    println!("Logs from your program will appear here!");

    // Uncomment the code below to pass the first stage
    //
    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();

    loop {
        match listener.accept() {
            Ok((stream, addr)) => handle_connection(stream, addr).unwrap(),
            Err(e) => println!("error: {}", e),
        }
    }
}

const BUFFER_SIZE: usize = 1024;

fn handle_connection(
    mut stream: std::net::TcpStream,
    addr: std::net::SocketAddr,
) -> std::io::Result<()> {
    println!(
        "accepted new connection from host {:?} and port {}",
        addr.ip(),
        addr.port()
    );

    'MAIN_LOOP: loop {
        let mut buffer = [0; BUFFER_SIZE];
        let mut cmd = String::new();

        'READ_LOOP: loop {
            let read = stream.read(&mut buffer)?;
            if let Ok(chunk) = String::from_utf8(buffer[0..read].to_vec()) {
                println!("read chunk: {chunk}");
                cmd.push_str(chunk.as_str());
                if read < BUFFER_SIZE {
                    break 'READ_LOOP;
                }
            } else {
                break 'MAIN_LOOP;
            }
        }

        println!("read raw command {cmd}");
        stream.write("+PONG\r\n".as_bytes())?;

    }
    Ok(())
}
