const RN: &str = "\r\n";

#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DataType {
    SimpleString { content: String },
    RespError { message: String },
    Integer { value: i64 },
    BulkString { len: usize, content: String },
    NullString,
    Array { len: usize, contents: Vec<DataType> },
    NullArray,
    Null,
}

#[allow(dead_code)]
impl DataType {
    fn parse_one(raw: String) -> (Self, String) {
        match raw.as_bytes()[0] {
            // ==================================== SIMPLE STRINGS ====================================
            // +<content>\r\n
            b'+' => {
                let (content, rest) = raw[1..]
                    .split_once(RN)
                    .map(|(c, r)| (c.into(), r.into()))
                    .expect("\r\n not found");

                (Self::SimpleString { content }, rest)
            }

            // ==================================== ERRORS ====================================
            // -Error message\r\n
            b'-' => {
                let (message, rest) = raw[1..]
                    .split_once(RN)
                    .map(|(c, r)| (c.into(), r.into()))
                    .expect("\r\n not found");

                (Self::RespError { message }, rest)
            }
            // ==================================== INTEGERS ====================================
            // :[<+|->]<value>\r\n
            b':' => {
                let (value, rest) = raw[1..]
                    .split_once(RN)
                    .map(|(v, r)| (v.parse::<i64>().expect("invalid value number"), r.into()))
                    .expect("\r\n not found");

                (Self::Integer { value }, rest)
            }
            // ==================================== BULK STRING ====================================
            // $<len>\r\n<content n bytes>\r\n
            b'$' => {
                let arr: Vec<String> = raw[1..].splitn(2, RN).map(|s| s.into()).collect();

                let (len, more) = (
                    arr.get(0)
                        .expect("invalid bulk string {raw}: expected at least 1 element")
                        .parse::<i64>()
                        .expect("invalid length in bulk string {raw}"),
                    arr.get(1),
                );

                match (len, more) {
                    (-1, Some(more)) => (Self::NullString, more.into()),
                    (len, Some(more)) if len as usize <= more.len() => {
                        let len = len as usize;
                        let content = (more[0..len]).to_string();
                        let rest = (more[len..])
                            .strip_prefix(RN)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                panic!("invalid final part {}", more[len..].to_string())
                            });

                        (Self::BulkString { len, content }, rest)
                    }
                    _ => panic!("invalid bulk string {raw}"),
                }
            }
            // ==================================== ARRAYS ====================================
            // *<number-of-elements>\r\n<element-1>...<element-n>
            b'*' => {
                let (arr_len, rest) = raw[1..]
                    .split_once(RN)
                    .map(|(l, r)| {
                        (
                            l.parse::<i64>().expect("invalid array len in {raw}"),
                            r.to_string(),
                        )
                    })
                    .expect("\r\n not found in {raw}");
                match (arr_len, rest) {
                    (-1, rest) => (Self::NullArray, rest),
                    (len, rest) if len > -1 => {
                        let len = len as usize;
                        let mut contents = Vec::<DataType>::new();
                        let mut rest = rest;
                        let mut data_type: Self;

                        for _ in 0..len {
                            (data_type, rest) = Self::parse_one(rest);
                            contents.push(data_type);
                        }

                        (Self::Array { len, contents }, rest)
                    }
                    _ => panic!("invalid array literal {raw}"),
                }
            }
            // ==================================== NULLS ====================================
            // _\r\n
            b'_' => (
                Self::Null,
                raw[1..]
                    .split_once(RN)
                    .map(|(_, s)| s.into())
                    .expect("expecting at least one \r\n"),
            ),
            _ => panic!("invalid RESP literal {raw}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_string() {
        // Test simple string
        assert_eq!(
            DataType::parse_one("+ciao\r\n".to_string()),
            (
                DataType::SimpleString {
                    content: "ciao".to_string()
                },
                "".into()
            ),
            "Failed to parse simple string"
        );

        // Test simple string with remainder
        assert_eq!(
            DataType::parse_one("+ciao\r\n-hola\r\n".to_string()),
            (
                DataType::SimpleString {
                    content: "ciao".to_string()
                },
                "-hola\r\n".into()
            ),
            "Failed to parse simple string with remainder"
        );

        // Test empty simple string
        assert_eq!(
            DataType::parse_one("+\r\n".to_string()),
            (
                DataType::SimpleString {
                    content: "".to_string()
                },
                "".into()
            ),
            "Failed to parse empty simple string"
        );
    }

    #[test]
    fn test_parse_err() {
        // Test simple error
        assert_eq!(
            DataType::parse_one("-ciao\r\n".to_string()),
            (
                DataType::RespError {
                    message: "ciao".to_string()
                },
                "".into()
            ),
            "Failed to parse simple error"
        );

        // Test simple error with remainder
        assert_eq!(
            DataType::parse_one("-ciao\r\n+pippo\r\n".to_string()),
            (
                DataType::RespError {
                    message: "ciao".to_string()
                },
                "+pippo\r\n".into()
            ),
            "Failed to parse simple error with remainder"
        );

        // Test error with spaces
        assert_eq!(
            DataType::parse_one("-ERR unknown command 'foobar'\r\n".to_string()),
            (
                DataType::RespError {
                    message: "ERR unknown command 'foobar'".to_string()
                },
                "".into()
            ),
            "Failed to parse error with spaces"
        );
    }

    #[test]
    fn test_parse_int() {
        // Test positive integer
        assert_eq!(
            DataType::parse_one(":+12\r\n".to_string()),
            (DataType::Integer { value: 12 }, "".into()),
            "Failed to parse positive integer"
        );

        // Test negative integer
        assert_eq!(
            DataType::parse_one(":-12\r\n".to_string()),
            (DataType::Integer { value: -12 }, "".into()),
            "Failed to parse negative integer"
        );

        // Test zero
        assert_eq!(
            DataType::parse_one(":0\r\n".to_string()),
            (DataType::Integer { value: 0 }, "".into()),
            "Failed to parse zero integer"
        );

        // Test positive integer with remainder
        assert_eq!(
            DataType::parse_one(":+12\r\n+OK\r\n".to_string()),
            (DataType::Integer { value: 12 }, "+OK\r\n".into()),
            "Failed to parse positive integer with remainder"
        );

        // Test negative integer with remainder
        assert_eq!(
            DataType::parse_one(":-12\r\n+OK\r\n".to_string()),
            (DataType::Integer { value: -12 }, "+OK\r\n".into()),
            "Failed to parse negative integer with remainder"
        );

        // Test large number (i64 max)
        assert_eq!(
            DataType::parse_one(":9223372036854775807\r\n".to_string()),
            (
                DataType::Integer {
                    value: 9223372036854775807
                },
                "".into()
            ),
            "Failed to parse i64 max value"
        );

        // Test large negative number (i64 min)
        assert_eq!(
            DataType::parse_one(":-9223372036854775808\r\n".to_string()),
            (
                DataType::Integer {
                    value: -9223372036854775808
                },
                "".into()
            ),
            "Failed to parse i64 min value"
        );
    }

    #[test]
    fn test_bulk_str() {
        // Test standard bulk string
        assert_eq!(
            DataType::parse_one("$4\r\nciao\r\n".to_string()),
            (
                DataType::BulkString {
                    len: 4,
                    content: "ciao".to_string()
                },
                "".into()
            ),
            "Failed to parse standard bulk string"
        );

        // Test Null Bulk String
        assert_eq!(
            DataType::parse_one("$-1\r\n".to_string()),
            (DataType::NullString, "".into()),
            "Failed to parse null bulk string"
        );

        // Test standard bulk string with remainder
        assert_eq!(
            DataType::parse_one("$4\r\nciao\r\n+Ok\r\n".to_string()),
            (
                DataType::BulkString {
                    len: 4,
                    content: "ciao".to_string()
                },
                "+Ok\r\n".into()
            ),
            "Failed to parse standard bulk string with remainder"
        );

        // Test Null Bulk String with remainder
        assert_eq!(
            DataType::parse_one("$-1\r\n+Ok\r\n".to_string()),
            (DataType::NullString, "+Ok\r\n".into()),
            "Failed to parse null bulk string with remainder"
        );

        // Test Empty Bulk String
        assert_eq!(
            DataType::parse_one("$0\r\n\r\n".to_string()),
            (
                DataType::BulkString {
                    len: 0,
                    content: "".to_string()
                },
                "".into()
            ),
            "Failed to parse empty bulk string"
        );

        // Test bulk string containing \r\n
        assert_eq!(
            DataType::parse_one("$8\r\nfoo\r\nbar\r\n".to_string()),
            (
                DataType::BulkString {
                    len: 8,
                    content: "foo\r\nbar".to_string()
                },
                "".into()
            ),
            "Failed to parse bulk string containing CRLF"
        );
    }

    #[test]
    fn test_parse_null() {
        // Test RESP3 Null
        assert_eq!(
            DataType::parse_one("_\r\n".to_string()),
            (DataType::Null, "".into()),
            "Failed to parse RESP3 Null"
        );

        // Test RESP3 Null with remainder
        assert_eq!(
            DataType::parse_one("_\r\n:123\r\n".to_string()),
            (DataType::Null, ":123\r\n".into()),
            "Failed to parse RESP3 Null with remainder"
        );
    }

    #[test]
    fn test_parse_array() {
        // Test Null Array
        assert_eq!(
            DataType::parse_one("*-1\r\n".to_string()),
            (DataType::NullArray, "".into()),
            "Failed to parse Null Array"
        );

        // Test Empty Array
        assert_eq!(
            DataType::parse_one("*0\r\n".to_string()),
            (
                DataType::Array {
                    len: 0,
                    contents: vec![]
                },
                "".into()
            ),
            "Failed to parse Empty Array"
        );

        // Test Null Array with remainder
        assert_eq!(
            DataType::parse_one("*-1\r\n+OK\r\n".to_string()),
            (DataType::NullArray, "+OK\r\n".into()),
            "Failed to parse Null Array with remainder"
        );

        // Test Array of two bulk strings
        let array_str = "*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n";
        let expected_data = DataType::Array {
            len: 2,
            contents: vec![
                DataType::BulkString {
                    len: 3,
                    content: "foo".to_string(),
                },
                DataType::BulkString {
                    len: 3,
                    content: "bar".to_string(),
                },
            ],
        };
        assert_eq!(
            DataType::parse_one(array_str.to_string()),
            (expected_data, "".into()),
            "Failed to parse array of two bulk strings"
        );

        // Test Array of mixed types with remainder
        let mixed_array_str = "*3\r\n:1\r\n+Hello\r\n-Error\r\n$4\r\nciao\r\n";
        let expected_mixed_data = DataType::Array {
            len: 3,
            contents: vec![
                DataType::Integer { value: 1 },
                DataType::SimpleString {
                    content: "Hello".to_string(),
                },
                DataType::RespError {
                    message: "Error".to_string(),
                },
            ],
        };
        assert_eq!(
            DataType::parse_one(mixed_array_str.to_string()),
            (expected_mixed_data, "$4\r\nciao\r\n".into()), // Corrected remainder
            "Failed to parse array of mixed types with remainder"
        );

        // Test Nested Array
        let nested_array_str = "*2\r\n*2\r\n+One\r\n:2\r\n$3\r\nend\r\n";
        let expected_nested_data = DataType::Array {
            len: 2,
            contents: vec![
                DataType::Array {
                    len: 2,
                    contents: vec![
                        DataType::SimpleString {
                            content: "One".to_string(),
                        },
                        DataType::Integer { value: 2 },
                    ],
                },
                DataType::BulkString {
                    len: 3,
                    content: "end".to_string(),
                },
            ],
        };
        assert_eq!(
            DataType::parse_one(nested_array_str.to_string()),
            (expected_nested_data, "".into()),
            "Failed to parse nested array"
        );

        // Test array containing nulls
        let array_with_nulls = "*3\r\n$-1\r\n*-1\r\n_\r\n";
        let expected_nulls_data = DataType::Array {
            len: 3,
            contents: vec![DataType::NullString, DataType::NullArray, DataType::Null],
        };
        assert_eq!(
            DataType::parse_one(array_with_nulls.to_string()),
            (expected_nulls_data, "".into()),
            "Failed to parse array containing nulls"
        );
    }
}
