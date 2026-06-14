use std::io::{self, Error};

use crate::resp::{self, RespType};

#[derive(Debug)]
pub enum Command {
    PING,
    ECHO { to_echo: String },
}

impl TryFrom<resp::RespType> for Command {
    type Error = io::Error;

    fn try_from(value: resp::RespType) -> Result<Self, Self::Error> {
        match value {
            resp::RespType::Array { elements } => {
                match elements.first().ok_or(io::Error::new(
                    io::ErrorKind::Other,
                    "Redis Commands should be RESP arrays",
                ))? {
                    RespType::BulkString { data } => {
                        let cmd = String::from_utf8(data.clone())
                            .map_err(|_| {
                                io::Error::new(
                                    io::ErrorKind::Other,
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

                                return Ok(Self::ECHO { to_echo: msg });
                            }
                            "PING" => return Ok(Self::PING),
                            _ => {
                                return Err(io::Error::new(io::ErrorKind::Other, "NYI"));
                            }
                        }
                    }
                    _ => todo!(),
                };
            }
            _ => Err(io::Error::new(
                io::ErrorKind::Other,
                "Redis Commands should be RESP arrays",
            )),
        }
    }
}
