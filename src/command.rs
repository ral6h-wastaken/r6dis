use std::io;

use crate::resp::{self, RespType};

#[derive(Debug)]
pub enum Command {
    Ping,
    Echo { to_echo: String },
    Set { key: String, value: String },
    Get { key: String },
}

impl TryFrom<resp::RespType> for Command {
    type Error = io::Error;

    fn try_from(value: resp::RespType) -> Result<Self, Self::Error> {
        match value {
            resp::RespType::Array { elements } => {
                match elements
                    .first()
                    .ok_or(io::Error::other("Redis Commands should be RESP arrays"))?
                {
                    RespType::BulkString { data } => {
                        let cmd = String::from_utf8(data.clone())
                            .map_err(|_| io::Error::other("Redis Commands should be RESP arrays"))?
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
                            }
                            "PING" => Ok(Self::Ping),
                            "SET" => parse_set_cmd(elements),
                            "GET" => parse_get_cmd(elements),
                            _ => Err(io::Error::other("NYI")),
                        }
                    }
                    _ => todo!(),
                }
            }
            _ => Err(io::Error::other("Redis Commands should be RESP arrays")),
        }
    }
}

fn parse_get_cmd(elements: Vec<RespType>) -> Result<Command, io::Error> {
    let key = elements
        .get(1)
        .map(|k| match k {
            RespType::BulkString { data } => String::from_utf8(data.clone()).ok(),
            _ => None,
        })
        .flatten()
        .ok_or(io::Error::other(
            "Invalid GET command: absent or invalid key",
        ))?;

    Ok(Command::Get { key })
}

fn parse_set_cmd(elements: Vec<RespType>) -> Result<Command, io::Error> {
    let key = elements
        .get(1)
        .map(|k| match k {
            RespType::BulkString { data } => String::from_utf8(data.clone()).ok(),
            _ => None,
        })
        .flatten()
        .ok_or(io::Error::other(
            "Invalid SET command: absent or invalid key",
        ))?;

    let value = elements
        .get(2)
        .map(|k| match k {
            RespType::BulkString { data } => String::from_utf8(data.clone()).ok(),
            _ => None,
        })
        .flatten()
        .ok_or(io::Error::other(
            "Invalid SET command: absent or invalid value",
        ))?;


    Ok(Command::Set { key, value })
}
