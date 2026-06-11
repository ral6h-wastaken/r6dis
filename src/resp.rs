use std::{io, str::FromStr};

const SEPARATOR: &[u8; 2] = b"\r\n";

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
        let c = match value.get(0) {
            None => {
                return Err(io::Error::new(io::ErrorKind::Other, "Empty RESP literal"));
            }
            Some(c) => c,
        };

        match c {
            b'+' => parse_simple_string(&value),
            b'$' => parse_bulk_string(&value),
            b'*' => parse_array(&value),
            _ => todo!("Unsupported prefix {c}"),
        }
    }
}

fn parse_array(value: &[u8]) -> Result<RespType, io::Error> {
    todo!("parse resp array")
}

fn parse_bulk_string(value: &[u8]) -> Result<RespType, io::Error> {
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

            if !remaining.ends_with(SEPARATOR) {
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

fn parse_simple_string(value: &[u8]) -> Result<RespType, io::Error> {
    if !value.ends_with(SEPARATOR) {
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

#[cfg(test)]
mod test {
    use std::io;

    use crate::resp::{RespType, test};

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

    #[test]
    fn resptype_parse_array() {
        let array = RespType::Array {
            len: 2,
            elements: vec![
                RespType::SimpleString {
                    content: "ciao".into(),
                },
                RespType::BulkString {
                    length: 4,
                    data: "ciao".as_bytes().to_vec(),
                },
            ],
        };

        let array_literal = b"*2\r\n+ciao\r\n$4\r\nciao\r\n";

        assert_eq!(array, RespType::try_from(array_literal.to_vec()).unwrap());

        let empty = RespType::Array {
            len: 0,
            elements: vec![],
        };

        let empty_array_literal = b"*0\r\n";

        assert_eq!(
            empty,
            RespType::try_from(empty_array_literal.to_vec()).unwrap()
        );
    }
}

