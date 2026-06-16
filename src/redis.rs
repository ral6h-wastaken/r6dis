use std::collections::HashMap;
use std::{ops::Add as _, time};

use crate::{command::Command, resp::RespType};

#[derive(Debug)]
pub struct Redis {
    kv_store: HashMap<String, StoredValue>,
    list_store: HashMap<String, Vec<String>>,
}

#[derive(Debug)]
struct StoredValue {
    data: String,
    ttl: Option<time::Instant>,
}

impl Redis {
    pub fn new() -> Self {
        Self {
            kv_store: HashMap::new(),
            list_store: HashMap::new(),
        }
    }

    pub fn handle_command(&mut self, cmd: Command) -> Result<RespType, String> {
        match cmd {
            Command::Ping => Ok(RespType::SimpleString {
                content: "PONG".into(),
            }),
            Command::Echo { to_echo } => Ok(RespType::BulkString {
                data: to_echo.into_bytes(),
            }),
            Command::Set {
                key,
                value,
                options,
            } => {
                let value = StoredValue {
                    data: value,
                    ttl: options.expire().map(|exp| time::Instant::now().add(exp)),
                };

                self.kv_store.insert(key, value);

                Ok(RespType::SimpleString {
                    content: "OK".into(),
                })
            }
            Command::Get { key } => match self.kv_store.get(key.as_str()) {
                Some(v) if v.ttl.is_some_and(|ttl| time::Instant::now().gt(&ttl)) => {
                    self.kv_store.remove(&key);
                    Ok(RespType::NullBulkString)
                }
                Some(v) => Ok(RespType::BulkString {
                    data: v.data.as_bytes().to_vec(),
                }),
                None => Ok(RespType::NullBulkString),
            },
            Command::RPush { key, mut elements } => {
                let entry = self
                    .list_store
                    .entry(key)
                    .and_modify(|l| l.append(&mut elements))
                    .or_insert(elements);

                Ok(RespType::Integer {
                    integer: entry.len() as i64,
                })
            }
            Command::LRange { key, start, stop } => {
                // Out of range indexes will not produce an error.
                // If start is larger than the end of the list, an empty list is returned.
                // If stop is larger than the actual end of the list, Redis will treat it like the last element of the list.
                match self.list_store.get(&key) {
                    Some(list) if !list.is_empty() => {
                        let compute_real_index = |idx: i64| -> usize {
                            if idx < 0 {
                                (list.len() as i64).saturating_add(idx).max(0) as usize
                            } else {
                                idx as usize
                            }
                        };

                        let start = compute_real_index(start);
                        let stop = compute_real_index(stop).min(list.len().saturating_sub(1));

                        let elements = if start > stop {
                            vec![]
                        } else {
                            list[start..=stop]
                                .iter()
                                .map(|val| RespType::BulkString {
                                    data: val.as_bytes().to_vec(),
                                })
                                .collect()
                        };

                        Ok(RespType::Array { elements })
                    }
                    _ => Ok(RespType::Array { elements: vec![] }),
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{command::Command, resp::RespType};

    #[test]
    fn test_handle_lrange() {
        let mut rds = super::Redis::new();

        let key = String::from("test key");

        let rpush_cmd = crate::command::Command::RPush {
            key: key.clone(),
            elements: vec![
                String::from("first"),
                String::from("second"),
                String::from("third"),
                String::from("fourth"),
                String::from("fifth"),
            ],
        };

        let sz = rds.handle_command(rpush_cmd).unwrap();
        assert_eq!(sz, RespType::Integer { integer: 5 });

        //test stop > len
        let lrange_cmd = Command::LRange {
            key: key.clone(),
            start: 0,
            stop: 1234,
        };
        let res = rds.handle_command(lrange_cmd);
        let expected = RespType::Array {
            elements: vec![
                RespType::BulkString {
                    data: String::from("first").into_bytes(),
                },
                RespType::BulkString {
                    data: String::from("second").into_bytes(),
                },
                RespType::BulkString {
                    data: String::from("third").into_bytes(),
                },
                RespType::BulkString {
                    data: String::from("fourth").into_bytes(),
                },
                RespType::BulkString {
                    data: String::from("fifth").into_bytes(),
                },
            ],
        };

        assert!(res.is_ok_and(|val| {
            assert_eq!(val, expected);
            return true;
        }));

        //negative start and stop, non empty
        let lrange_cmd = Command::LRange {
            key: key.clone(),
            start: -3,
            stop: -1,
        };
        let res = rds.handle_command(lrange_cmd);
        let expected = RespType::Array {
            elements: vec![
                RespType::BulkString {
                    data: String::from("third").into_bytes(),
                },
                RespType::BulkString {
                    data: String::from("fourth").into_bytes(),
                },
                RespType::BulkString {
                    data: String::from("fifth").into_bytes(),
                },
            ],
        };

        assert!(res.is_ok_and(|val| {
            assert_eq!(val, expected);
            return true;
        }));

        let lrange_cmd = Command::LRange {
            key: key.clone(),
            start: -1,
            stop: -2,
        };
        let res = rds.handle_command(lrange_cmd);
        let expected = RespType::Array { elements: vec![] };

        assert!(res.is_ok_and(|val| {
            assert_eq!(val, expected);
            return true;
        }));
    }
}
