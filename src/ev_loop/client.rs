use std::{
    io::{self, Read as _, Write as _},
    net::TcpStream,
    time,
};

use crate::redis::RedisError;

#[derive(Debug)]
pub(super) struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
    state: ClientState,
}

#[derive(Debug)]
enum ClientState {
    Ready,
    Wait,
    TimedWait(time::Instant),
}

impl Client {
    pub(super) fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            buffer: vec![],
            state: ClientState::Ready,
        }
    }

    pub(crate) fn read_raw_cmd(&mut self) -> Vec<u8> {
        let mut buf = [0u8; 512];

        match self.stream.read(&mut buf) {
            Err(err) => {
                let errmsg = format!("Could not read from socket, got error {err}");
                errmsg.into_bytes()
            }
            Ok(read) => buf[..read].to_vec(),
        }
    }

    pub(crate) fn step(&mut self, outcome: Result<crate::resp::RespType, RedisError>) {
        match outcome {
            Ok(response) => self.buffer.append(&mut response.serialize()),
            Err(_) => todo!(),
        }
    }

    pub(crate) fn flush(&mut self) -> Result<(), io::Error> {
        if !self.buffer.is_empty() {
            self.stream.write_all(&self.buffer)?;
            self.buffer.clear();
        };

        Ok(())
    }

    pub(super) fn stream(&self) -> &TcpStream {
        &self.stream
    }
}
