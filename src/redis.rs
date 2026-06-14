use std::collections::HashMap;

use crate::{command::Command, resp::RespType};

#[derive(Debug)]
pub struct Redis {
    store: HashMap<String, String>,
}

impl Redis {
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
        }
    }

    pub fn handle_command(&mut self, cmd: Command) -> Result<RespType, String> {
        match cmd {
            Command::Ping => Ok(RespType::SimpleString {
                content: "PONG".into(),
            }),
            Command::Echo { to_echo } => Ok(RespType::BulkString {
                data: to_echo.into_bytes(),
            }),
            Command::Set { key, value } => {
                self.store.insert(key, value);
                Ok(RespType::SimpleString {
                    content: "OK".into(),
                })
            }
            Command::Get { key } => match self.store.get(key.as_str()) {
                Some(v) => Ok(RespType::BulkString {
                    data: v.as_bytes().to_vec(),
                }),
                None => Ok(RespType::NullBulkString),
            },
        }
    }
}
