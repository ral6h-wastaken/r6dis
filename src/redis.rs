use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Instant;
use std::{ops::Add as _, time};

use crate::{command::Command, resp::RespType};

#[derive(Debug, Default)]
pub struct Redis {
    kv_store: HashMap<String, StoredValue>,
    list_store: HashMap<String, Vec<String>>,
    //key is the client_id, the values are the keys that blpop is waiting for
    //once we need to be waiting on more than one command we will introduce an enum
    //that will, for blpop, encapsulate this vec
    pub waiting_clients: HashMap<i32, WaitingState>,
    pub to_be_notified: Vec<(i32, String)>,
    blocking_keys: HashMap<String, Vec<i32>>,
    pub ready: Vec<(i32, RespType)>,
}

#[derive(Debug, Default)]
pub struct WaitingState {
    keys: Vec<String>,
    timeout: Option<time::Instant>,
}

#[allow(unused)] //TODO [LS]: remove the allow once we use the failure error
#[derive(Debug)]
pub enum RedisError {
    Failure(String),
    WouldBlock,
}

#[derive(Debug)]
struct StoredValue {
    data: String,
    ttl: Option<time::Instant>,
}

impl Redis {
    pub fn handle_command(&mut self, cmd: Command, client_id: i32) -> Result<RespType, RedisError> {
        println!("Handling command {cmd:?} from client {client_id}");

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
                    .entry(key.clone())
                    .and_modify(|l| l.append(&mut elements))
                    .or_insert(elements);

                //"notify" waiting clients
                if let Some(clients) = self.blocking_keys.get_mut(&key).filter(|v| !v.is_empty()) {
                    let longest = clients.remove(0);
                    //a client can only be waiting for a single event at a time, if it stops
                    //waiting then it is removed from the to_be_notified map
                    self.to_be_notified.push((longest, key.clone()));
                }

                Ok(RespType::Integer {
                    integer: entry.len() as i64,
                })
            }
            Command::LPush { key, elements } => {
                let entry = self
                    .list_store
                    .entry(key.clone())
                    .and_modify(|l| {
                        elements.iter().for_each(|el| l.insert(0, el.clone()));
                    })
                    .or_insert(elements.into_iter().rev().collect());

                //"notify" waiting clients
                if let Some(clients) = self.blocking_keys.get_mut(&key).filter(|v| !v.is_empty()) {
                    let longest = clients.remove(0);
                    //a client can only be waiting for a single event at a time, if it stops
                    //waiting then it is removed from the to_be_notified map
                    self.to_be_notified.push((longest, key.clone()));
                }

                Ok(RespType::Integer {
                    integer: entry.len() as i64,
                })
            }
            Command::LLen { key } => {
                let len = self.list_store.get(&key).map_or(0, |l| l.len());

                Ok(RespType::Integer {
                    integer: len as i64,
                })
            }
            Command::LPop { key, count } => self.lpop(key, count),
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
            Command::BlPop { key, timeout } => {
                match self.list_store.get(&key).and_then(|l| l.first()) {
                    Some(_) => {
                        let val = self.list_store.get_mut(&key).unwrap().remove(0);
                        Ok(RespType::Array {
                            elements: vec![
                                RespType::BulkString {
                                    data: key.into_bytes(),
                                },
                                RespType::BulkString {
                                    data: val.into_bytes(),
                                },
                            ],
                        })
                    }
                    None => {
                        let waiting_state = self.waiting_clients.entry(client_id).or_default();

                        waiting_state.keys.push(key.clone());
                        waiting_state.timeout = timeout.map(|dur| Instant::now() + dur);

                        self.blocking_keys
                            .entry(key)
                            .or_insert(vec![])
                            .push(client_id);

                        Err(RedisError::WouldBlock)
                    }
                }
            }
            Command::Type { key } => {
                //TODO [LS]: only handle this case for now, refactor will be needed later
                if let Some(_) = self.kv_store.get(&key) {
                    Ok(RespType::SimpleString { content: "string".into() })
                } else {
                    Ok(RespType::SimpleString { content: "none".into() })
                }
            }
            Command::ErrorCmd { msg } => Ok(RespType::SimpleError { content: msg }),
        }
    }

    pub(crate) fn remove_waiting(&mut self, client_id: &i32) {
        if let Some(idx) = self
            .to_be_notified
            .iter()
            .position(|(id, _)| id == client_id)
        {
            self.to_be_notified.remove(idx);
        }

        if let Some(idx) = self.ready.iter().position(|(id, _)| id == client_id) {
            self.ready.remove(idx);
        }

        if let Some(state) = self.waiting_clients.remove(client_id) {
            for key in state.keys {
                if let Some(blocked) = self.blocking_keys.get_mut(&key)
                    && let Some(idx) = blocked.iter().position(|bl| bl == client_id)
                {
                    blocked.remove(idx);
                }
            }
        }
    }

    pub(crate) fn remove_expired(&mut self) -> Vec<i32> {
        let expired: Vec<i32> = self
            .waiting_clients
            .iter()
            .filter(|(_, v)| v.timeout.is_some_and(|t| time::Instant::now() >= t))
            .map(|(k, _)| *k)
            .collect();

        for cl in &expired {
            self.remove_waiting(cl);
        }

        expired
    }

    pub(crate) fn compute_ready(&mut self) {
        while let Some((client_id, key)) = self.to_be_notified.pop() {
            let cl_key = key.clone();
            let resp = self.lpop(key, 1).map(|val| RespType::Array {
                elements: vec![RespType::BulkString {
                    data: cl_key.as_bytes().to_vec(),
                }, val],
            })
            .unwrap();

            self.ready.push((client_id, resp));
        }
    }

    fn lpop(&mut self, key: String, count: usize) -> Result<RespType, RedisError> {
        let mut pop_list = Vec::<String>::with_capacity(count);

        while let Some(list) = self.list_store.get_mut(&key).filter(|v| !v.is_empty())
            && pop_list.len() < count
        {
            pop_list.push(list.remove(0));
        }

        match pop_list.len() {
            0 => Ok(RespType::NullBulkString),
            1 => Ok(RespType::BulkString {
                data: pop_list[0].as_bytes().to_vec(),
            }),
            _ => Ok(RespType::Array {
                elements: pop_list
                    .iter()
                    .map(|popped| RespType::BulkString {
                        data: popped.as_bytes().to_vec(),
                    })
                    .collect(),
            }),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::{command::Command, resp::RespType};

    #[test]
    fn test_handle_lrange() {
        let mut rds = super::Redis::default();

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

        let sz = rds.handle_command(rpush_cmd, 0).unwrap();
        assert_eq!(sz, RespType::Integer { integer: 5 });

        //test stop > len
        let lrange_cmd = Command::LRange {
            key: key.clone(),
            start: 0,
            stop: 1234,
        };
        let res = rds.handle_command(lrange_cmd, 0);
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
        let res = rds.handle_command(lrange_cmd, 0);
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
        let res = rds.handle_command(lrange_cmd, 0);
        let expected = RespType::Array { elements: vec![] };

        assert!(res.is_ok_and(|val| {
            assert_eq!(val, expected);
            return true;
        }));
    }
}
