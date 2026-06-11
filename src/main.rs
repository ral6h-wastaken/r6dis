#![allow(unused_imports)]
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd as _,
    str::FromStr as _,
};

mod ev_loop;
mod poll;

use crate::{poll::Poller, redis::RespCommand};

fn main() -> std::io::Result<()> {
    const PORT: u32 = 6379;

    let listener = TcpListener::bind(format!("127.0.0.1:{PORT}").as_str())
        .expect(format!("Error while binding to port {PORT}").as_str());

    listener.set_nonblocking(true).map_err(|err| {
        eprintln!("Could not set listener to non blocking mode");
        err
    })?;

    let poller = Poller::new(&listener)?;
    // let looper = crate::ev_loop::EventLoop::new(listener, poller);
    let mut looper = crate::ev_loop::EventLoop::new(listener, poller);

    looper.run()
}

mod redis {
    use std::{io, str::FromStr};

    const SEPARATOR: &str = "\r\n";

    pub enum RespCommand {
        PING,
        ECHO { msg: String },
    }

    impl FromStr for RespCommand {
        type Err = io::Error;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            todo!()
        }
    }

    #[derive(Debug, Eq, PartialEq)]
    pub enum RespType {
        //*<number-of-elements>\r\n<element-1>...<element-n>
        Array { len: usize, elements: Vec<RespType> },
        //+<content>\r\n
        SimpleString { content: String },
        //$<length>\r\n<data>\r\n
        BulkString { length: usize, data: Vec<u8> },
        NullBulkString,
    }

    impl TryFrom<Vec<u8>> for RespType {
        type Error = io::Error;

        fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
            match value.get(0).unwrap() {
                b'+' => {
                    if !value.ends_with(SEPARATOR.as_bytes()) {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "Simple string must end with \\r\\n",
                        ));
                    }
                    let len = value.len();
                    let value = &value[1..len - 2];

                    if value.contains(&b'\r') || value.contains(&b'\n') {
                        return Err(io::Error::new(
                            io::ErrorKind::Other,
                            "Simple string must not contain either \\r or \\n",
                        ));
                    }

                    match String::from_utf8(value.to_vec()) {
                        Ok(content) => Ok(RespType::SimpleString { content }),
                        Err(_) => Err(io::Error::new(
                            io::ErrorKind::Other,
                            "Simple string must be a valid utf8 encoded string",
                        )),
                    }
                }

                b'$' => {
                    let value = &value[1..];

                    if let None = value.get(0) {
                        return Err(io::Error::new(io::ErrorKind::Other, "Invalid bulk string"));
                    }

                    match value.get(0).unwrap() {
                        b'-' => {
                            return if b"-1\r\n" == value {
                                Ok(RespType::NullBulkString)
                            } else {
                                Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "Only null bulk strings can start with -",
                                ))
                            };
                        }
                        _ => {
                            let mut len_barr = Vec::<u8>::new();
                            let mut found_r = false;

                            let value_iter = value.iter();

                            'len_parsing: for c in value_iter {
                                match c {
                                    b'\r' => found_r = true,
                                    b'\n' => {
                                        if !found_r {
                                            return Err(io::Error::new(
                                                io::ErrorKind::Other,
                                                "Invalid bulk string length",
                                            ));
                                        }

                                        break 'len_parsing;
                                    }
                                    _ => {
                                        //avoid cases where \r and \n are not near each other
                                        //we could use a peekable iterator but this will do it for
                                        //now
                                        if found_r {
                                            return Err(io::Error::new(
                                                io::ErrorKind::Other,
                                                "Invalid bulk string length",
                                            ));
                                        }
                                        //check if in 0-9 ascii range
                                        if *c >= 48u8 && *c <= 57u8 {
                                            len_barr.push(*c);
                                        } else {
                                            return Err(io::Error::new(
                                                io::ErrorKind::Other,
                                                "Invalid bulk string length",
                                            ));
                                        }
                                    }
                                }
                            }

                            let bytes_len = len_barr.len();
                            if bytes_len == 0 {
                                return Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "Invalid bulk string length",
                                ));
                            }

                            let len_str = match String::from_utf8(len_barr) {
                                Ok(l) => l,
                                Err(_) => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::Other,
                                        "Invalid bulk string length",
                                    ));
                                }
                            };

                            let length = usize::from_str_radix(&len_str, 10)
                                .expect("We have checked previously that every byte we push corresponds to valid ascii in the range 0-9");

                            //skipping initial number + last 2 bytes of \r\n
                            let remaining = &value[2 + bytes_len..];

                            if !remaining.ends_with(b"\r\n") {
                                return Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "Bulk strings must end with \\r\\n",
                                ));
                            }

                            let data = remaining[..remaining.len() - 2].to_vec();
                            if data.len() != length {
                                return Err(io::Error::new(
                                    io::ErrorKind::Other,
                                    "Bulk string declared length does not match actual length",
                                ));
                            }

                            return Ok(RespType::BulkString { length, data });
                        }
                    }
                }

                b'*' => todo!("parse array"),
                _ => todo!(),
            }
        }
    }

    #[cfg(test)]
    mod test {
        use std::io;

        use crate::redis::{RespType, test};

        #[test]
        fn resptype_parse_simplestring() {
            let expected = RespType::SimpleString {
                content: "ciao".into(),
            };

            assert_eq!(expected, RespType::try_from(b"+ciao\r\n".to_vec()).unwrap());

            let invalid_terminated = RespType::try_from(b"+ciao".to_vec());

            assert!(invalid_terminated.is_err());

            let err = invalid_terminated.err().unwrap();
            assert_eq!(
                "Simple string must end with \\r\\n".to_string(),
                err.to_string()
            );

            let invalid_content = RespType::try_from(b"+ci\rao\r\n".to_vec());

            assert!(invalid_content.is_err());

            let err = invalid_content.err().unwrap();
            assert_eq!(
                "Simple string must not contain either \\r or \\n".to_string(),
                err.to_string()
            );

            let invalid_utf8 = RespType::try_from(b"+\xA4\r\n".to_vec());

            assert!(invalid_utf8.is_err());

            let err = invalid_utf8.err().unwrap();
            assert_eq!(
                "Simple string must be a valid utf8 encoded string".to_string(),
                err.to_string()
            );
        }

        #[test]
        fn resptype_parse_bulkstring() {
            let non_null = RespType::BulkString {
                length: 4,
                data: b"ciao".to_vec(),
            };

            assert_eq!(
                non_null,
                RespType::try_from(b"$4\r\nciao\r\n".to_vec()).unwrap()
            );

            let null = RespType::NullBulkString;
            assert_eq!(null, RespType::try_from(b"$-1\r\n".to_vec()).unwrap());

            let invalid_null = RespType::try_from(b"$-4ciao\r\n".to_vec());

            assert!(invalid_null.is_err());

            let err = invalid_null.err().unwrap();
            assert_eq!(
                "Only null bulk strings can start with -".to_string(),
                err.to_string()
            );

            let invalid_len = RespType::try_from(b"$c\r\n".to_vec());

            assert!(invalid_len.is_err());

            let err = invalid_len.err().unwrap();
            assert_eq!("Invalid bulk string length".to_string(), err.to_string());

            let invalid_len = RespType::try_from(b"$12\r2\r\n".to_vec());

            assert!(invalid_len.is_err());

            let err = invalid_len.err().unwrap();
            assert_eq!("Invalid bulk string length".to_string(), err.to_string());

            let invalid_len = RespType::try_from(b"$\r\nciao\r\n".to_vec());

            assert!(invalid_len.is_err());

            let err = invalid_len.err().unwrap();
            assert_eq!("Invalid bulk string length".to_string(), err.to_string());

            let invalid_len = RespType::try_from(b"$12\r\nciao\r\n".to_vec());

            assert!(invalid_len.is_err());

            let err = invalid_len.err().unwrap();
            assert_eq!(
                "Bulk string declared length does not match actual length".to_string(),
                err.to_string()
            );

            let invalid_end = RespType::try_from(b"$4\r\nciao".to_vec());

            assert!(invalid_end.is_err());

            let err = invalid_end.err().unwrap();
            assert_eq!(
                "Bulk strings must end with \\r\\n".to_string(),
                err.to_string()
            );
        }
    }
}
