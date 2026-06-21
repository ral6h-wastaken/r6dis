use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Instant;
use std::{ops::Add as _, time};

use crate::{command::Command, resp::RespType};

#[derive(Debug, Default)]
pub struct Redis {
    store: HashMap<String, RedisType>,

    //map<client_id, (state, timeout)>
    waiting_clients: HashMap<i32, (WaitingState, Option<time::Instant>)>,
    //map<client_id, event>
    to_be_notified: Vec<(i32, NotificationEvent)>,

    blpop_blocking_keys: HashMap<String, Vec<i32>>,

    pub ready: Vec<(i32, RespType)>,
}

#[derive(Debug)]
pub enum NotificationEvent {
    BlPopEvent { key: String },
}

#[derive(Debug)]
pub enum WaitingState {
    BlPop { keys: Vec<String> },
}

#[derive(Debug)]
struct StoredValue {
    data: String,
    ttl: Option<time::Instant>,
}

#[derive(Debug)]
//string, list, set, zset, hash, stream, and vectorset
enum RedisType {
    String { value: StoredValue },
    List { elements: Vec<String> },
    Stream { elements: Vec<StreamElement> },
}

#[derive(Debug)]
struct StreamElement {
    id: String,
    data: HashMap<String, String>,
}

#[allow(unused)] //TODO [LS]: remove the allow once we use the failure error
#[derive(Debug)]
pub enum RedisError {
    Failure(String),
    WouldBlock,
}

impl Redis {
    pub fn handle_command(&mut self, cmd: Command, client_id: i32) -> Result<RespType, RedisError> {
        println!("Handling command {cmd:?} from client {client_id}");

        match cmd {
            Command::Ping => handle_ping(),
            Command::Echo { to_echo } => handle_echo(to_echo),
            Command::Set {
                key,
                value,
                options,
            } => self.handle_set(key, value, options),
            Command::Get { key } => self.handle_get(key),
            Command::RPush { key, elements } => self.handle_rpush(key, elements),
            Command::LPush { key, elements } => self.handle_lpush(key, elements),
            Command::LLen { key } => self.handle_llen(key),
            Command::LPop { key, count } => self.handle_lpop(&key, count),
            Command::LRange { key, start, stop } => self.handle_lrange(key, start, stop),
            Command::BlPop { keys, timeout } => self.handle_blpop(client_id, keys, timeout),
            Command::Type { key } => self.handle_type(key),
            Command::XAdd { key, id, elements } => self.handle_xadd(key, id, elements),
            Command::ErrorCmd { msg } => handle_error(msg),
        }
    }

    fn handle_type(&mut self, key: String) -> Result<RespType, RedisError> {
        //TODO [LS]: only handle this case for now, refactor will be needed later
        if let Some(t) = self.store.get(&key) {
            match t {
                RedisType::String { value: _ } => Ok(RespType::SimpleString {
                    content: "string".into(),
                }),
                RedisType::List { elements: _ } => Ok(RespType::SimpleString {
                    content: "list".into(),
                }),
                RedisType::Stream { elements: _ } => Ok(RespType::SimpleString {
                    content: "stream".into(),
                }),
            }
        } else {
            Ok(RespType::SimpleString {
                content: "none".into(),
            })
        }
    }

    fn handle_blpop(
        &mut self,
        client_id: i32,
        keys: Vec<String>,
        timeout: Option<time::Duration>,
    ) -> Result<RespType, RedisError> {
        for key in keys.iter() {
            if !self.ensure_type(key, "list") {
                return Ok(RespType::SimpleError {
                    content: "WRONGTYPE Operation against a key holding the wrong kind of value"
                        .into(),
                });
            }

            match self.store.get(key).and_then(|t| match t {
                RedisType::List { elements } => elements.first(),
                _ => panic!("Illegal state"),
            }) {
                Some(_) => {
                    let val = self
                        .store
                        .get_mut(key)
                        .map(|t| match t {
                            RedisType::List { elements } => elements,
                            _ => panic!("Illegal state"),
                        })
                        .unwrap()
                        .remove(0);

                    //as soon as we find a matching entry we return ok
                    return Ok(RespType::Array {
                        elements: vec![
                            RespType::BulkString {
                                data: key.clone().into_bytes(),
                            },
                            RespType::BulkString {
                                data: val.into_bytes(),
                            },
                        ],
                    });
                }
                None => {
                    //we continue iterating, maybe some other key in keys will have an element
                    //available
                }
            }
        }

        let timeout = timeout.map(|dur| Instant::now() + dur);

        for key in keys.iter() {
            self.blpop_blocking_keys
                .entry(key.clone())
                .or_insert(vec![])
                .push(client_id);
        }

        //a given client can only be blocked on a given state at once
        self.waiting_clients
            .insert(client_id, (WaitingState::BlPop { keys }, timeout));

        Err(RedisError::WouldBlock)
    }

    fn handle_lrange(
        &mut self,
        key: String,
        start: i64,
        stop: i64,
    ) -> Result<RespType, RedisError> {
        if !self.ensure_type(&key, "list") {
            return Ok(RespType::SimpleError {
                content: "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
            });
        }

        // Out of range indexes will not produce an error.
        // If start is larger than the end of the list, an empty list is returned.
        // If stop is larger than the actual end of the list, Redis will treat it like the last element of the list.
        match self.store.get(&key) {
            Some(RedisType::List { elements }) if !elements.is_empty() => {
                let compute_real_index = |idx: i64| -> usize {
                    if idx < 0 {
                        (elements.len() as i64).saturating_add(idx).max(0) as usize
                    } else {
                        idx as usize
                    }
                };

                let start = compute_real_index(start);
                let stop = compute_real_index(stop).min(elements.len().saturating_sub(1));

                let elements = if start > stop {
                    vec![]
                } else {
                    elements[start..=stop]
                        .iter()
                        .map(|val| RespType::BulkString {
                            data: val.as_bytes().to_vec(),
                        })
                        .collect()
                };

                Ok(RespType::Array { elements })
            }
            Some(RedisType::List { elements: _ }) => Ok(RespType::Array { elements: vec![] }),
            _ => panic!("Illegal state"),
        }
    }

    fn handle_llen(&mut self, key: String) -> Result<RespType, RedisError> {
        if !self.ensure_type(&key, "list") {
            return Ok(RespType::SimpleError {
                content: "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
            });
        }

        let len = self.store.get(&key).map_or(0, |t| match t {
            RedisType::List { elements } => elements.len(),
            _ => panic!("Illegal state"),
        });

        Ok(RespType::Integer {
            integer: len as i64,
        })
    }

    fn handle_lpush(&mut self, key: String, elements: Vec<String>) -> Result<RespType, RedisError> {
        if !self.ensure_type(&key, "list") {
            return Ok(RespType::SimpleError {
                content: "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
            });
        }

        let entry = self
            .store
            .entry(key.clone())
            .and_modify(|l| match l {
                RedisType::List { elements: els } => {
                    elements.iter().for_each(|el| els.insert(0, el.clone()))
                }
                _ => panic!("Illegal state"),
            })
            .or_insert(RedisType::List {
                elements: elements.into_iter().rev().collect(),
            });
        //"notify" waiting clients
        if let Some(clients) = self
            .blpop_blocking_keys
            .get_mut(&key)
            .filter(|v| !v.is_empty())
        {
            let longest = clients.remove(0);
            //a client can only be waiting for a single event at a time, if it stops
            //waiting then it is removed from the to_be_notified map
            self.to_be_notified
                .push((longest, NotificationEvent::BlPopEvent { key: key.clone() }));
        }

        Ok(RespType::Integer {
            integer: match entry {
                RedisType::List { elements } => elements.len() as i64,
                _ => panic!("Illegal state"),
            },
        })
    }

    fn handle_rpush(
        &mut self,
        key: String,
        mut elements: Vec<String>,
    ) -> Result<RespType, RedisError> {
        if !self.ensure_type(&key, "list") {
            return Ok(RespType::SimpleError {
                content: "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
            });
        }

        let elements_len = elements.len();

        let entry = self
            .store
            .entry(key.clone())
            .and_modify(|l| match l {
                RedisType::List { elements: els } => els.append(&mut elements),
                _ => panic!("Illegal state"),
            })
            .or_insert(RedisType::List { elements });

        //"notify" waiting clients
        if let Some(clients) = self
            .blpop_blocking_keys
            .get_mut(&key)
            .filter(|v| !v.is_empty())
        {
            let mut notified = 0;
            while let Some(_) = clients.get(0)
                && notified < elements_len
            {
                let longest = clients.remove(0);
                //a client can only be waiting for a single event at a time, if it stops
                //waiting then it is removed from the to_be_notified map
                self.to_be_notified
                    .push((longest, NotificationEvent::BlPopEvent { key: key.clone() }));

                notified += 1;
            }
        }

        Ok(RespType::Integer {
            integer: match entry {
                RedisType::List { elements } => elements.len() as i64,
                _ => panic!("Illegal state"),
            },
        })
    }

    fn handle_get(&mut self, key: String) -> Result<RespType, RedisError> {
        if !self.ensure_type(&key, "string") {
            return Ok(RespType::SimpleError {
                content: "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
            });
        }

        match self.store.get(&key) {
            Some(RedisType::String { value: v })
                if v.ttl.is_some_and(|ttl| time::Instant::now().gt(&ttl)) =>
            {
                self.store.remove(&key);
                Ok(RespType::NullBulkString)
            }
            Some(RedisType::String { value: v }) => Ok(RespType::BulkString {
                data: v.data.as_bytes().to_vec(),
            }),
            Some(_) => {
                panic!("Should be unreachable, due to type check at the beginning of this function")
            }
            None => Ok(RespType::NullBulkString),
        }
    }

    fn handle_set(
        &mut self,
        key: String,
        value: String,
        options: crate::command::SetOptions,
    ) -> Result<RespType, RedisError> {
        if !self.ensure_type(&key, "string") {
            return Ok(RespType::SimpleError {
                content: "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
            });
        }

        let value = StoredValue {
            data: value,
            ttl: options.expire().map(|exp| time::Instant::now().add(exp)),
        };

        self.store.insert(key, RedisType::String { value });

        Ok(RespType::SimpleString {
            content: "OK".into(),
        })
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

        if let Some((state, _)) = self.waiting_clients.remove(client_id) {
            match state {
                WaitingState::BlPop { keys } => {
                    for key in keys {
                        if let Some(blocked) = self.blpop_blocking_keys.get_mut(&key)
                            && let Some(idx) = blocked.iter().position(|bl| bl == client_id)
                        {
                            blocked.remove(idx);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn remove_expired(&mut self) -> Vec<i32> {
        let expired: Vec<i32> = self
            .waiting_clients
            .iter()
            .filter(|(_, (_, timeout))| timeout.is_some_and(|t| time::Instant::now() >= t))
            .map(|(k, _)| *k)
            .collect();

        for cl in &expired {
            self.remove_waiting(cl);
        }

        expired
    }

    pub(crate) fn compute_ready(&mut self) {
        while let Some(_) = self.to_be_notified.first() {
            let (client_id, notification) = self.to_be_notified.remove(0);

            match notification {
                NotificationEvent::BlPopEvent { key } => {
                    let (state, _) = self
                        .waiting_clients
                        .remove(&client_id)
                        .expect("Invalid state: waiting client without keys");

                    //this check will be useful for when we'll introduce new
                    //blocking states in the future
                    #[allow(irrefutable_let_patterns)]
                    if let WaitingState::BlPop { keys } = state {
                        for k in keys {
                            self.blpop_blocking_keys
                                .entry(k)
                                .and_modify(|v| v.retain(|c| *c != client_id));
                        }
                    } else {
                        panic!(
                            "Invalid state: received blpop notification event but waiting state does not match"
                        )
                    }

                    let resp = self
                        .handle_lpop(&key, 1)
                        .map(|val| RespType::Array {
                            elements: vec![
                                RespType::BulkString {
                                    data: key.as_bytes().to_vec(),
                                },
                                val,
                            ],
                        })
                        .unwrap();

                    self.ready.push((client_id, resp));
                }
            }
        }
    }

    fn handle_lpop(&mut self, key: &str, count: usize) -> Result<RespType, RedisError> {
        if !self.ensure_type(key, "list") {
            return Ok(RespType::SimpleError {
                content: "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
            });
        }

        let mut pop_list = Vec::<String>::with_capacity(count);

        while let Some(list) = self.store.get_mut(key).and_then(|v| {
            if let RedisType::List { elements } = v
                && !elements.is_empty()
            {
                Some(elements)
            } else {
                None
            }
        }) && pop_list.len() < count
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

    fn ensure_type(&self, key: &str, wanted: &str) -> bool {
        match self.store.get(key) {
            Some(t) => match t {
                RedisType::String { value: _ } => wanted == "string",
                RedisType::List { elements: _ } => wanted == "list",
                RedisType::Stream { elements: _ } => wanted == "stream",
            },
            None => true,
        }
    }

    fn handle_xadd(
        &mut self,
        key: String,
        id: String,
        elements: Vec<(String, String)>,
    ) -> Result<RespType, RedisError> {
        if !self.ensure_type(&key, "stream") {
            return Ok(RespType::SimpleError {
                content: "WRONGTYPE Operation against a key holding the wrong kind of value".into(),
            });
        }

        let id_utf8 = id.as_bytes().to_vec();
        let data: HashMap<String, String> = elements.into_iter().collect();

        self.store
            .entry(key)
            .and_modify(|t| match t {
                RedisType::Stream { elements } => elements.push(StreamElement {
                    id: id.clone(),
                    data: data.clone(),
                }),
                _ => panic!("Illegal state"),
            })
            .or_insert(RedisType::Stream {
                elements: vec![StreamElement { id, data }],
            });

        Ok(RespType::BulkString { data: id_utf8 })
    }
}

fn handle_error(msg: String) -> Result<RespType, RedisError> {
    Ok(RespType::SimpleError { content: msg })
}

fn handle_echo(to_echo: String) -> Result<RespType, RedisError> {
    Ok(RespType::BulkString {
        data: to_echo.into_bytes(),
    })
}

fn handle_ping() -> Result<RespType, RedisError> {
    Ok(RespType::SimpleString {
        content: "PONG".into(),
    })
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
