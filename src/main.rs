#![allow(unused_imports)]
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;

mod parser;

#[allow(dead_code)]
enum Command {
    Ping,
    Echo(String)
}

#[allow(dead_code)]
impl Command {
    fn from(raw: parser::DataType) -> Self {
        todo!()
    }
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:6379").unwrap();

    loop {
        match listener.accept() {
            Ok((stream, addr)) => {
                std::thread::spawn(move || handle_connection(stream, addr).unwrap());
            }
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

    loop {
        let cmd = read_command(&mut stream)?;

        println!("read raw command {cmd}");
        match stream.write_all("+PONG\r\n".as_bytes()) {
            Ok(_) => (),
            Err(_) => break,
        }
    }

    Ok(())
}

fn read_command(stream: &mut std::net::TcpStream) -> Result<String, std::io::Error> {
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
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "read invalid utf 8 from the stream",
            ));
        }
    }
    Ok(cmd)
}
