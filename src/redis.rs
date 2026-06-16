use std::collections::HashMap;
use std::{ops::Add as _, time};

use crate::{command::Command, resp::RespType};

#[derive(Debug)]
pub struct Redis {
    store: HashMap<String, StoredValue>,
}

#[derive(Debug)]
struct StoredValue {
    data: String,
    ttl: Option<time::Instant>,
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
            Command::Set {
                key,
                value,
                options,
            } => {
                let value = StoredValue {
                    data: value,
                    ttl: options.expire().map(|exp| time::Instant::now().add(exp)),
                };

                self.store.insert(key, value);

                Ok(RespType::SimpleString {
                    content: "OK".into(),
                })
            }
            Command::Get { key } => match self.store.get(key.as_str()) {
                Some(v) if v.ttl.is_some_and(|ttl| time::Instant::now().gt(&ttl)) => {
                    self.store.remove(&key);
                    Ok(RespType::NullBulkString)
                }
                Some(v) => Ok(RespType::BulkString {
                    data: v.data.as_bytes().to_vec(),
                }),
                None => Ok(RespType::NullBulkString),
            },
            Command::Rpush { key, elements } => todo!(),
        }
    }
}
