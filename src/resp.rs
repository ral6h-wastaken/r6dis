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
        let mut cursor = 0;
        let len = value.len();

        if let Some(c) = value.get(cursor) {
            let (parsed, _) = match c {
                b'+' => parse_simple_string(&value, cursor+1)?,
                b'$' => parse_bulk_string(&value, cursor)?,
                b'*' => parse_array(&value, cursor)?,
                _ => todo!("Unsupported prefix {c}"),
            };

            return Ok(parsed);
        } 
        
        todo!()
    }
}

fn parse_array(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    todo!("parse resp array")
}

fn parse_bulk_string(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    todo!()
}

fn parse_simple_string(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {

    let end_idx = match find_separator_index(value, cursor) {
        Some(idx) => idx,
        None => return Err(io::Error::new(io::ErrorKind::Other, "Simple string must end with \\r\\n"))
    };

    let value = &value[cursor..end_idx];

    if value.contains(&b'\r') || value.contains(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Simple string must not contain either \\r or \\n",
        ));
    }

    match String::from_utf8(value.to_vec()) {
        Ok(content) => Ok((RespType::SimpleString { content }, end_idx + 2)),
        Err(_) => Err(io::Error::new(
            io::ErrorKind::Other,
            "Simple string must be a valid utf8 encoded string",
        )),
    }
}

fn find_separator_index(value: &[u8], cursor: usize) -> Option<usize> {
    let mut found_r = false;
    let mut r_idx = 0usize;

    for (i,c) in value[cursor..].iter().enumerate() {
        match c {
            b'\r' => {
                found_r = true;
                r_idx = cursor + i;     //with enumerate, i will always start at 0 thus we need to
                                        //pad it
            },
            b'\n' => { 
                if found_r {
                    return Some(r_idx)
                }
            },
            _ => found_r = false
        };
    }

    None
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
    //
    // #[test]
    // fn resptype_parse_array() {
    //     let array = RespType::Array {
    //         len: 2,
    //         elements: vec![
    //             RespType::SimpleString {
    //                 content: "ciao".into(),
    //             },
    //             RespType::BulkString {
    //                 length: 4,
    //                 data: "ciao".as_bytes().to_vec(),
    //             },
    //         ],
    //     };
    //
    //     let array_literal = b"*2\r\n+ciao\r\n$4\r\nciao\r\n";
    //
    //     assert_eq!(array, RespType::try_from(array_literal.to_vec()).unwrap());
    //
    //     let empty = RespType::Array {
    //         len: 0,
    //         elements: vec![],
    //     };
    //
    //     let empty_array_literal = b"*0\r\n";
    //
    //     assert_eq!(
    //         empty,
    //         RespType::try_from(empty_array_literal.to_vec()).unwrap()
    //     );
    // }
}

