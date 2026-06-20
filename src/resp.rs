use std::{fmt::Display, io, str::FromStr};

#[derive(Debug, Eq, PartialEq)]
pub enum RespType {
    //*<number-of-elements>\r\n<element-1>...<element-n>
    Array { elements: Vec<RespType> },
    //+<content>\r\n
    SimpleString { content: String },
    //-<content>\r\n
    SimpleError { content: String },
    //$<length>\r\n<data>\r\n
    BulkString { data: Vec<u8> },
    //:[<+|->]<value>\r\n
    Integer { integer: i64 },
    //$-1\r\n
    NullBulkString,
}

impl TryFrom<&[u8]> for RespType {
    type Error = io::Error;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let c = value.first().ok_or(io::Error::other("Empty value"))?;

        Ok(parse_single_value(value, c, 0)?.0)
    }
}

impl RespType {
    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::<u8>::new();

        match self {
            RespType::Array { elements } => {
                format!("*{}\r\n", elements.len())
                    .as_bytes()
                    .iter()
                    .for_each(|c| result.push(*c));

                elements
                    .iter()
                    .flat_map(|el| el.serialize())
                    .for_each(|e| result.push(e));
            }
            RespType::SimpleString { content } => {
                format!("+{}\r\n", content)
                    .as_bytes()
                    .iter()
                    .for_each(|c| result.push(*c));
            }
            RespType::SimpleError { content } => {
                format!("-{}\r\n", content)
                    .as_bytes()
                    .iter()
                    .for_each(|c| result.push(*c));
            }
            RespType::BulkString { data } => {
                format!("${}\r\n", data.len())
                    .as_bytes()
                    .iter()
                    .for_each(|c| result.push(*c));

                data.iter().for_each(|c| result.push(*c));

                b"\r\n".iter().for_each(|c| result.push(*c));
            }
            RespType::Integer { integer } => {
                format!(":{}\r\n", integer)
                    .as_bytes()
                    .iter()
                    .for_each(|c| result.push(*c));
            }
            RespType::NullBulkString => b"$-1\r\n".iter().for_each(|c| result.push(*c)),
        };

        result
    }
}

fn parse_single_value(value: &[u8], c: &u8, cursor: usize) -> Result<(RespType, usize), io::Error> {
    Ok(match c {
        b'+' => parse_simple_string(value, cursor + 1)?,
        b':' => parse_integer(value, cursor + 1)?,
        b'-' => parse_simple_error(value, cursor + 1)?,
        b'$' => parse_bulk_string(value, cursor + 1)?,
        b'*' => parse_array(value, cursor + 1)?,
        _ => todo!("Unsupported prefix {c}"),
    })
}

fn parse_integer(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    parse_simple_data(value, cursor, SimpleDataType::Integer)
}

fn parse_array(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    //*<number-of-elements>\r\n<element-1>...<element-n>
    let sep_idx =
        find_separator_index(value, cursor).ok_or(io::Error::other("Invalid array size"))?;

    let size = usize::from_str(
        String::from_utf8(value[cursor..sep_idx].to_vec())
            .map_err(|_| io::Error::other("Invalid array size"))?
            .as_str(),
    )
    .map_err(|_| io::Error::other("Invalid array size"))?;

    let mut cursor = sep_idx + 2;
    let mut elements: Vec<RespType> = Vec::with_capacity(size);

    while let Some(c) = value.get(cursor) {
        let (parsed, new_pos) = parse_single_value(value, c, cursor)?;
        elements.push(parsed);
        cursor = new_pos;
    }

    if size != elements.len() {
        return Err(io::Error::other(
            "Array declared size does not match actual size",
        ));
    }

    Ok((RespType::Array { elements }, cursor))
}

fn parse_bulk_string(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    let sep_idx = find_separator_index(value, cursor)
        .ok_or(io::Error::other("Invalid bulk string length"))?;

    let length = isize::from_str(
        String::from_utf8(value[cursor..sep_idx].to_vec())
            .map_err(|_| io::Error::other("Invalid bulk string length"))?
            .as_str(),
    )
    .map_err(|_| io::Error::other("Invalid bulk string length"))?;

    if length >= 0 {
        let length = length as usize;
        let cursor = sep_idx + 2;

        let data = value[cursor..cursor+length].to_vec();
        let cursor = cursor + length;

        if &value[cursor..=cursor+1] != b"\r\n" {
            return Err(io::Error::other("Bulk strings must end with \\r\\n"))
        }
        
        Ok((
            RespType::BulkString {
                data
            },
            cursor + 2,
        ))
    } else {
        if length != -1 {
            return Err(io::Error::other("Only null bulk strings can start with -"));
        }

        Ok((RespType::NullBulkString, sep_idx + 2))
    }
}

fn parse_simple_string(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    parse_simple_data(value, cursor, SimpleDataType::String)
}

fn parse_simple_error(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    parse_simple_data(value, cursor, SimpleDataType::Error)
}

fn parse_simple_data(
    value: &[u8],
    cursor: usize,
    data_type: SimpleDataType,
) -> Result<(RespType, usize), io::Error> {
    let end_idx = find_separator_index(value, cursor).ok_or(io::Error::other(format!(
        "Simple {} must end with \\r\\n",
        &data_type
    )))?;

    let value = &value[cursor..end_idx];

    if value.contains(&b'\r') || value.contains(&b'\n') {
        return Err(io::Error::other(format!(
            "Simple {} must not contain either \\r or \\n",
            &data_type
        )));
    }

    match (String::from_utf8(value.to_vec()), &data_type) {
        (Ok(content), SimpleDataType::String) => {
            Ok((RespType::SimpleString { content }, end_idx + 2))
        }
        (Ok(content), SimpleDataType::Error) => {
            Ok((RespType::SimpleError { content }, end_idx + 2))
        }
        (Ok(content), SimpleDataType::Integer) => {
            let integer = content
                .parse::<i64>()
                .map_err(|err| io::Error::other(format!("invalid integer value: {err}")))?;

            Ok((RespType::Integer { integer }, end_idx + 2))
        }
        (Err(_), _) => Err(io::Error::other(format!(
            "Simple {} must be a valid utf8 encoded string",
            &data_type
        ))),
    }
}

fn find_separator_index(value: &[u8], cursor: usize) -> Option<usize> {
    value[cursor..]
        .windows(2)
        .position(|w| w == b"\r\n")
        .map(|i| cursor + i)
}

enum SimpleDataType {
    Integer,
    String,
    Error,
}

impl Display for &SimpleDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SimpleDataType::String => "string",
                SimpleDataType::Error => "error",
                SimpleDataType::Integer => "integer",
            }
        )
    }
}

#[cfg(test)]
mod test {
    use crate::resp::RespType;

    #[test]
    fn resptype_parse_integer() {
        let expected = RespType::Integer { integer: 69420 };

        assert_eq!(
            expected,
            RespType::try_from(":+69420\r\n".as_bytes()).unwrap()
        );

        let expected = RespType::Integer { integer: -69420 };

        assert_eq!(
            expected,
            RespType::try_from(":-69420\r\n".as_bytes()).unwrap()
        );

        let invalid_terminated = RespType::try_from(":12".as_bytes());

        assert!(invalid_terminated.is_err());

        let err = invalid_terminated.err().unwrap();
        assert_eq!(
            "Simple integer must end with \\r\\n".to_string(),
            err.to_string()
        );

        let invalid_content = RespType::try_from(":ci\rao\r\n".as_bytes());

        assert!(invalid_content.is_err());

        let err = invalid_content.err().unwrap();
        assert_eq!(
            "Simple integer must not contain either \\r or \\n".to_string(),
            err.to_string()
        );

        let invalid_utf8 = RespType::try_from(b":\xA4\r\n".as_slice());

        assert!(invalid_utf8.is_err());

        let err = invalid_utf8.err().unwrap();
        assert_eq!(
            "Simple integer must be a valid utf8 encoded string".to_string(),
            err.to_string()
        );

        let invalid_i64 = RespType::try_from(
            b":10000000000000000000000000000000000000000000000000\r\n".as_slice(),
        );

        assert!(invalid_i64.is_err());

        let err = invalid_i64.err().unwrap();
        assert!(err.to_string().starts_with("invalid integer value:"));
    }

    #[test]
    fn resptype_parse_simplestring() {
        let expected = RespType::SimpleString {
            content: "ciao".into(),
        };

        assert_eq!(
            expected,
            RespType::try_from("+ciao\r\n".as_bytes()).unwrap()
        );

        let invalid_terminated = RespType::try_from("+ciao".as_bytes());

        assert!(invalid_terminated.is_err());

        let err = invalid_terminated.err().unwrap();
        assert_eq!(
            "Simple string must end with \\r\\n".to_string(),
            err.to_string()
        );

        let invalid_content = RespType::try_from("+ci\rao\r\n".as_bytes());

        assert!(invalid_content.is_err());

        let err = invalid_content.err().unwrap();
        assert_eq!(
            "Simple string must not contain either \\r or \\n".to_string(),
            err.to_string()
        );

        let invalid_utf8 = RespType::try_from(b"+\xA4\r\n".as_slice());

        assert!(invalid_utf8.is_err());

        let err = invalid_utf8.err().unwrap();
        assert_eq!(
            "Simple string must be a valid utf8 encoded string".to_string(),
            err.to_string()
        );
    }

    #[test]
    fn resptype_serialize_integer() {
        let to_ser = RespType::Integer { integer: 43 };
        assert_eq!(to_ser.serialize(), b":43\r\n");

        let to_ser = RespType::Integer { integer: -43 };
        assert_eq!(to_ser.serialize(), b":-43\r\n");
    }

    #[test]
    fn resptype_serialize_simplestring() {
        let to_ser = RespType::SimpleString {
            content: "ciao".into(),
        };

        assert_eq!(to_ser.serialize(), b"+ciao\r\n");
    }

    #[test]
    fn resptype_parse_simpleerr() {
        let expected = RespType::SimpleError {
            content: "ciao".into(),
        };

        assert_eq!(
            expected,
            RespType::try_from("-ciao\r\n".as_bytes()).unwrap()
        );

        let invalid_terminated = RespType::try_from("-ciao".as_bytes());

        assert!(invalid_terminated.is_err());

        let err = invalid_terminated.err().unwrap();
        assert_eq!(
            "Simple error must end with \\r\\n".to_string(),
            err.to_string()
        );

        let invalid_content = RespType::try_from("-ci\rao\r\n".as_bytes());

        assert!(invalid_content.is_err());

        let err = invalid_content.err().unwrap();
        assert_eq!(
            "Simple error must not contain either \\r or \\n".to_string(),
            err.to_string()
        );

        let invalid_utf8 = RespType::try_from(b"-\xA4\r\n".as_slice());

        assert!(invalid_utf8.is_err());

        let err = invalid_utf8.err().unwrap();
        assert_eq!(
            "Simple error must be a valid utf8 encoded string".to_string(),
            err.to_string()
        );
    }

    #[test]
    fn resptype_ser_simpleerr() {
        let to_ser = RespType::SimpleError {
            content: "ciao".into(),
        };

        assert_eq!(to_ser.serialize(), b"-ciao\r\n");
    }

    #[test]
    fn resptype_parse_bulkstring() {
        let non_null = RespType::BulkString {
            data: b"ciao".to_vec(),
        };

        assert_eq!(
            non_null,
            RespType::try_from("$4\r\nciao\r\n".as_bytes()).unwrap()
        );

        let null = RespType::NullBulkString;
        assert_eq!(null, RespType::try_from("$-1\r\n".as_bytes()).unwrap());

        let invalid_null = RespType::try_from("$-4\r\n".as_bytes());

        assert!(invalid_null.is_err());

        let err = invalid_null.err().unwrap();
        assert_eq!(
            "Only null bulk strings can start with -".to_string(),
            err.to_string()
        );

        let invalid_len = RespType::try_from("$c\r\n".as_bytes());

        assert!(invalid_len.is_err());

        let err = invalid_len.err().unwrap();
        assert_eq!("Invalid bulk string length".to_string(), err.to_string());

        let invalid_len = RespType::try_from("$12\r2\r\n".as_bytes());

        assert!(invalid_len.is_err());

        let err = invalid_len.err().unwrap();
        assert_eq!("Invalid bulk string length".to_string(), err.to_string());

        let invalid_len = RespType::try_from("$\r\nciao\r\n".as_bytes());

        assert!(invalid_len.is_err());

        let err = invalid_len.err().unwrap();
        assert_eq!("Invalid bulk string length".to_string(), err.to_string());

        let invalid_end = RespType::try_from("$4\r\nciaone".as_bytes());

        assert!(invalid_end.is_err());

        let err = invalid_end.err().unwrap();
        assert_eq!(
            "Bulk strings must end with \\r\\n".to_string(),
            err.to_string()
        );
    }

    #[test]
    fn resptype_serialize_bulkstring() {
        let non_null = RespType::BulkString {
            data: b"ciao".to_vec(),
        };

        assert_eq!(non_null.serialize(), b"$4\r\nciao\r\n");

        let null = RespType::NullBulkString;
        assert_eq!(null.serialize(), b"$-1\r\n");
    }

    #[test]
    fn resptype_parse_array() {
        let array = RespType::Array {
            elements: vec![
                RespType::SimpleString {
                    content: "ciao".into(),
                },
                RespType::BulkString {
                    data: "ciao".as_bytes().to_vec(),
                },
            ],
        };

        let array_literal = "*2\r\n+ciao\r\n$4\r\nciao\r\n";

        assert_eq!(array, RespType::try_from(array_literal.as_bytes()).unwrap());

        let empty = RespType::Array { elements: vec![] };

        let empty_array_literal = "*0\r\n";

        assert_eq!(
            empty,
            RespType::try_from(empty_array_literal.as_bytes()).unwrap()
        );

        let invalid_size = "*3\r\n+ciao\r\n$4\r\nciao\r\n";
        let error = RespType::try_from(invalid_size.as_bytes());

        assert!(error.is_err());

        let err = error.err().unwrap();

        assert_eq!(
            "Array declared size does not match actual size".to_string(),
            err.to_string()
        );
    }

    #[test]
    fn resptype_serialize_array() {
        let array = RespType::Array {
            elements: vec![
                RespType::SimpleString {
                    content: "ciao".into(),
                },
                RespType::BulkString {
                    data: "ciao".as_bytes().to_vec(),
                },
            ],
        };
        let array_literal = b"*2\r\n+ciao\r\n$4\r\nciao\r\n";

        assert_eq!(array.serialize(), array_literal);

        let empty = RespType::Array { elements: vec![] };
        let empty_array_literal = b"*0\r\n";

        assert_eq!(empty.serialize(), empty_array_literal);
    }
}
