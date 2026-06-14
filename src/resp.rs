use std::{io, str::FromStr};

#[derive(Debug, Eq, PartialEq)]
pub enum RespType {
    //*<number-of-elements>\r\n<element-1>...<element-n>
    Array { elements: Vec<RespType> },
    //+<content>\r\n
    SimpleString { content: String },
    //$<length>\r\n<data>\r\n
    BulkString { data: Vec<u8> },
    NullBulkString,
}

impl TryFrom<Vec<u8>> for RespType {
    type Error = io::Error;

    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        let c = value
            .first()
            .ok_or(io::Error::new(io::ErrorKind::Other, "Empty value"))?;

        Ok(parse_single_value(&value, c, 0)?.0)
    }
}

fn parse_single_value(value: &[u8], c: &u8, cursor: usize) -> Result<(RespType, usize), io::Error> {
    Ok(match c {
        b'+' => parse_simple_string(&value, cursor + 1)?,
        b'$' => parse_bulk_string(&value, cursor + 1)?,
        b'*' => parse_array(&value, cursor + 1)?,
        _ => todo!("Unsupported prefix {c}"),
    })
}

fn parse_array(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    //*<number-of-elements>\r\n<element-1>...<element-n>
    let sep_idx = find_separator_index(value, cursor)
        .ok_or(io::Error::new(io::ErrorKind::Other, "Invalid array size"))?;

    let size = usize::from_str(
        String::from_utf8(value[cursor..sep_idx].to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid array size"))?
            .as_str(),
    )
    .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid array size"))?;

    let mut cursor = sep_idx + 2;
    let mut elements: Vec<RespType> = Vec::with_capacity(size);

    while let Some(c) = value.get(cursor) {
        let (parsed, new_pos) = parse_single_value(value, c, cursor)?;
        elements.push(parsed);
        cursor = new_pos;
    }

    if size != elements.len() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Array declared size does not match actual size",
        ));
    }

    Ok((RespType::Array { elements }, cursor))
}

fn parse_bulk_string(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    let sep_idx = find_separator_index(value, cursor).ok_or(io::Error::new(
        io::ErrorKind::Other,
        "Invalid bulk string length",
    ))?;

    let length = isize::from_str(
        String::from_utf8(value[cursor..sep_idx].to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid bulk string length"))?
            .as_str(),
    )
    .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid bulk string length"))?;

    if length >= 0 {
        let length = length as usize;
        let cursor = sep_idx + 2;

        let sep_idx = find_separator_index(value, cursor).ok_or(io::Error::new(
            io::ErrorKind::Other,
            "Bulk strings must end with \\r\\n",
        ))?;

        if (cursor + length) != sep_idx {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Bulk string declared length does not match actual length",
            ));
        }

        Ok((
            RespType::BulkString {
                data: value[cursor..sep_idx].to_vec(),
            },
            sep_idx + 2,
        ))
    } else {
        if length != -1 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Only null bulk strings can start with -",
            ));
        }

        Ok((RespType::NullBulkString, sep_idx + 2))
    }
}

fn parse_simple_string(value: &[u8], cursor: usize) -> Result<(RespType, usize), io::Error> {
    let end_idx = find_separator_index(value, cursor).ok_or(io::Error::new(
        io::ErrorKind::Other,
        "Simple string must end with \\r\\n",
    ))?;

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
    value[cursor..]
        .windows(2)
        .position(|w| w == b"\r\n")
        .map(|i| cursor + i)
}

#[cfg(test)]
mod test {
    use crate::resp::RespType;
    use std::io;

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
            data: b"ciao".to_vec(),
        };

        assert_eq!(
            non_null,
            RespType::try_from(b"$4\r\nciao\r\n".to_vec()).unwrap()
        );

        let null = RespType::NullBulkString;
        assert_eq!(null, RespType::try_from(b"$-1\r\n".to_vec()).unwrap());

        let invalid_null = RespType::try_from(b"$-4\r\n".to_vec());

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

        assert_eq!(array, RespType::try_from(array_literal.to_vec()).unwrap());

        let empty = RespType::Array { elements: vec![] };

        let empty_array_literal = b"*0\r\n";

        assert_eq!(
            empty,
            RespType::try_from(empty_array_literal.to_vec()).unwrap()
        );

        let invalid_size = b"*3\r\n+ciao\r\n$4\r\nciao\r\n";
        let error = RespType::try_from(invalid_size.to_vec());

        assert!(error.is_err());

        let err = error.err().unwrap();

        assert_eq!(
            "Array declared size does not match actual size".to_string(),
            err.to_string()
        );
    }
}
