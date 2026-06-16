use core::time;
use std::io;

use crate::resp::{self, RespType};

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Ping,
    Echo {
        to_echo: String,
    },
    Set {
        key: String,
        value: String,
        options: SetOptions,
    },
    Get {
        key: String,
    },
    Rpush {
        key: String,
        elements: Vec<String>,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct SetOptions {
    expire: Option<time::Duration>,
}

impl SetOptions {
    pub fn expire(&self) -> Option<std::time::Duration> {
        self.expire
    }
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
                            "ECHO" => parse_echo_cmd(&elements),
                            "PING" => Ok(Self::Ping),
                            "SET" => parse_set_cmd(&elements),
                            "GET" => parse_get_cmd(&elements),
                            "RPUSH" => parse_rpush_cmd(&elements),
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

fn parse_rpush_cmd(raw_elements: &[RespType]) -> Result<Command, io::Error> {
    if raw_elements.len() < 3 {
        return Err(io::Error::other(
            "Invalid RPUSH command: absent or invalid key",
        ));
    }

    let key = raw_elements
        .get(1)
        .and_then(|k| match k {
            RespType::BulkString { data } => String::from_utf8(data.clone()).ok(),
            _ => None,
        })
        .ok_or(io::Error::other(
            "Invalid RPUSH command: absent or invalid key",
        ))?;

    let mut elements = Vec::with_capacity(raw_elements.len() - 2);
    for v in raw_elements.iter().skip(2) {
        let elem = match v {
            RespType::BulkString { data } => String::from_utf8(data.clone()).map_err(|err| {
                io::Error::other(format!("Invalid UTF8 ecoded RPUSH argument: {err}"))
            })?,
            _ => {
                return Err(io::Error::other(
                    "RPUSH arguments must be RESP bulk strings",
                ));
            }
        };

        elements.push(elem);
    }

    Ok(Command::Rpush { key, elements })
}

fn parse_echo_cmd(elements: &[RespType]) -> Result<Command, io::Error> {
    let msg = elements.get(1).unwrap();
    let msg = match msg {
        RespType::BulkString { data } => String::from_utf8(data.clone()).unwrap(),
        _ => todo!(),
    };
    Ok(Command::Echo { to_echo: msg })
}

fn parse_get_cmd(elements: &[RespType]) -> Result<Command, io::Error> {
    let key = elements
        .get(1)
        .and_then(|k| match k {
            RespType::BulkString { data } => String::from_utf8(data.clone()).ok(),
            _ => None,
        })
        .ok_or(io::Error::other(
            "Invalid GET command: absent or invalid key",
        ))?;

    Ok(Command::Get { key })
}

fn parse_set_cmd(elements: &[RespType]) -> Result<Command, io::Error> {
    let key = elements
        .get(1)
        .and_then(|k| match k {
            RespType::BulkString { data } => String::from_utf8(data.clone()).ok(),
            _ => None,
        })
        .ok_or(io::Error::other(
            "Invalid SET command: absent or invalid key",
        ))?;

    let value = elements
        .get(2)
        .and_then(|k| match k {
            RespType::BulkString { data } => String::from_utf8(data.clone()).ok(),
            _ => None,
        })
        .ok_or(io::Error::other(
            "Invalid SET command: absent or invalid value",
        ))?;

    Ok(Command::Set {
        key,
        value,
        options: parse_set_options(elements)?,
    })
}

fn parse_set_options(elements: &[RespType]) -> Result<SetOptions, io::Error> {
    // The SET command supports a set of options that modify its behavior:
    //
    //     NX -- Only set the key if it does not already exist.
    //     XX -- Only set the key if it already exists.
    //     IFEQ ifeq-value -- Set the key’s value and expiration only if its current value is equal to ifeq-value. If the key doesn’t exist, it won’t be created.
    //     IFNE ifne-value -- Set the key’s value and expiration only if its current value is not equal to ifne-value. If the key doesn’t exist, it will be created.
    //     IFDEQ ifeq-digest -- Set the key’s value and expiration only if the hash digest of its current value is equal to ifeq-digest. If the key doesn’t exist, it won’t be created. See the Hash Digest section below for more information.
    //     IFDNE ifne-digest -- Set the key’s value and expiration only if the hash digest of its current value is not equal to ifne-digest. If the key doesn’t exist, it will be created. See the Hash Digest section below for more information.
    //     GET -- Return the old string stored at key, or nil if key did not exist. An error is returned and SET aborted if the value stored at key is not a string.
    //     EX seconds -- Set the specified expire time, in seconds (a positive integer).
    //     PX milliseconds -- Set the specified expire time, in milliseconds (a positive integer).
    //     EXAT timestamp-seconds -- Set the specified Unix time at which the key will expire, in seconds (a positive integer).
    //     PXAT timestamp-milliseconds -- Set the specified Unix time at which the key will expire, in milliseconds (a positive integer).
    //     KEEPTTL -- Retain the time to live associated with the key.

    let mut options = SetOptions { expire: None };
    if elements.get(3).is_none() {
        return Ok(options);
    }

    let elements = &elements[3..];

    for elem in elements.windows(2) {
        match elem {
            [
                RespType::BulkString { data: option },
                RespType::BulkString { data: value },
            ] => {
                let option = String::from_utf8(option.clone())
                    .map_err(|_| io::Error::other("Invalid utf8 when parsing set option"))?;

                let value = String::from_utf8(value.clone())
                    .map_err(|_| io::Error::other("Invalid utf8 when parsing set option"))?;

                match option.to_ascii_uppercase().as_str() {
                    "EX" => {
                        let ttl = value.parse::<u64>().map_err(|err| {
                            io::Error::other(format!("invalid expiry value: {err}"))
                        })?;

                        options.expire = Some(std::time::Duration::from_secs(ttl));
                    }
                    "PX" => {
                        let ttl = value.parse::<u64>().map_err(|err| {
                            io::Error::other(format!("invalid expiry value: {err}"))
                        })?;

                        options.expire = Some(std::time::Duration::from_millis(ttl));
                    }
                    _ => { /*Skip iteration, it's probably a value which we already considered before*/
                    }
                }
            }
            _ => {
                return Err(io::Error::other(
                    "Set options and values must be encoded as bulk strings",
                ));
            }
        }
    }

    Ok(options)
}

#[cfg(test)]
mod test {
    use core::time;

    use crate::{
        command::{Command, SetOptions, parse_set_cmd},
        resp::RespType,
    };

    #[test]
    fn test_parse_set_command_no_options() {
        let expected = Command::Set {
            key: "key".into(),
            value: "value".into(),
            options: SetOptions { expire: None },
        };

        let elements = vec![
            RespType::BulkString {
                data: "SET".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "key".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "value".as_bytes().to_vec(),
            },
        ];
        let parsed = parse_set_cmd(&elements);

        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();

        assert_eq!(expected, parsed);
    }

    #[test]
    fn test_parse_set_command_with_expire() {
        let expected = Command::Set {
            key: "key".into(),
            value: "value".into(),
            options: SetOptions {
                expire: Some(time::Duration::from_millis(1000)),
            },
        };

        let elements = vec![
            RespType::BulkString {
                data: "SET".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "key".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "value".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "px".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "1000".as_bytes().to_vec(),
            },
        ];
        let parsed = parse_set_cmd(&elements);

        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();

        assert_eq!(expected, parsed);

        let elements = vec![
            RespType::BulkString {
                data: "SET".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "key".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "value".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "ex".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "1".as_bytes().to_vec(),
            },
        ];
        let parsed = parse_set_cmd(&elements);

        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();

        assert_eq!(expected, parsed);
    }

    #[test]
    fn test_parse_set_command_invalid_expire() {
        let elements = vec![
            RespType::BulkString {
                data: "SET".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "key".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "value".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "px".as_bytes().to_vec(),
            },
            RespType::BulkString {
                data: "ciccio".as_bytes().to_vec(),
            },
        ];
        let parsed = parse_set_cmd(&elements);

        assert!(parsed.is_err());
        assert!(
            parsed
                .err()
                .unwrap()
                .to_string()
                .starts_with("invalid expiry value: ")
        )
    }
}
