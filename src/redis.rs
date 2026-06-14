use crate::{command::Command, resp::RespType};

#[derive(Debug)]
pub struct Redis {
    capacity: usize,
}

impl Redis {
    pub fn new() -> Self {
        Self { capacity: 0 }
    }

    pub fn handle_command(&mut self, cmd: Command) -> Result<RespType, String> {
        match cmd {
            Command::Ping => Ok(RespType::SimpleString {
                content: "PONG".into(),
            }),
            Command::Echo { to_echo } => Ok(RespType::BulkString {
                data: to_echo.into_bytes(),
            }),
        }
    }
}
