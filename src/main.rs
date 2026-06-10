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
        ARRAY { len: usize, elements: Vec<RespType> },
        //+<content>\r\n
        SIMPLE_STRING { content: String },
        //$<length>\r\n<data>\r\n
        BULK_STRING { length: usize, data: String },
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
                        Ok(content) => Ok(RespType::SIMPLE_STRING { content }),
                        Err(_) => Err(io::Error::new(
                            io::ErrorKind::Other,
                            "Simple string must be a valid utf8 encoded string",
                        )),
                    }
                }
                b'$' => todo!("parse bulk string"),
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
            let expected = RespType::SIMPLE_STRING {
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
    }
}
