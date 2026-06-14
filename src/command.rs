use std::io;

use crate::resp::{self, RespType};

#[derive(Debug)]
pub enum Command {
    Ping,
    Echo { to_echo: String },
}

impl TryFrom<resp::RespType> for Command {
    type Error = io::Error;

    fn try_from(value: resp::RespType) -> Result<Self, Self::Error> {
        match value {
            resp::RespType::Array { elements } => {
                match elements.first().ok_or(io::Error::other(
                    "Redis Commands should be RESP arrays",
                ))? {
                    RespType::BulkString { data } => {
                        let cmd = String::from_utf8(data.clone())
                            .map_err(|_| {
                                io::Error::other(
                                    "Redis Commands should be RESP arrays",
                                )
                            })?
                            .to_ascii_uppercase();

                        match cmd.as_str() {
                            "ECHO" => {
                                let msg = elements.get(1).unwrap();
                                let msg = match msg {
                                    RespType::BulkString { data } => {
                                        String::from_utf8(data.clone()).unwrap()
                                    }
                                    _ => todo!(),
                                };

                                Ok(Self::Echo { to_echo: msg })
                            },
                            "PING" => Ok(Self::Ping),
                            _ => {
                                Err(io::Error::other("NYI"))
                            }
                        }
                    },
                    _ => todo!(),
                }
            },
            _ => Err(io::Error::other(
                "Redis Commands should be RESP arrays",
            )),
        }
    }
}
