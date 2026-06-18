use std::{
    collections::HashMap,
    io::{self},
    net::TcpListener,
    os::fd::AsRawFd,
};

mod client;

use libc::{EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLOUT, EPOLLRDHUP};

use crate::{command::Command, poll::Poller, redis::Redis, resp::RespType};

#[derive(Debug)]
pub struct EventLoop {
    redis: Redis,
    listener: TcpListener,
    clients: HashMap<i32, client::Client>,
    poller: Poller,
}

impl EventLoop {
    pub fn new(listener: TcpListener, poller: Poller) -> Self {
        Self {
            redis: Redis::default(),
            listener,
            poller,
            clients: HashMap::new(),
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        println!("Starting main event loop. Listening for connections at port 6379");
        let listener_fd = self.listener.as_raw_fd() as u64;

        'event_loop: loop {
            // println!("Looper state {self:?}");

            //loop over all waiting and for each expired send back a null bulk str
            for client_id in self.redis.remove_expired() {
                if let Some(cl) = self.clients.get_mut(&client_id) {
                    println!("Timeout occurred for {client_id:?}");
                    cl.send(RespType::NullBulkString);
                }
            }

            self.redis.compute_ready();

            while let Some((client_id, response)) = self.redis.ready.pop() {
                if let Some(cl) = self.clients.get_mut(&client_id) {
                    println!("Sending response {response:?} to client {client_id}");
                    cl.send(response);
                    println!("Looper state {self:?}");
                }
            }

            let events = self.poller.poll(-1)?;

            for ev in events {
                let descriptor = ev.u64;

                if descriptor == listener_fd && ((EPOLLIN as u32) & ev.events != 0) {
                    //ev.events will be a | mask of all
                    //the events that are ready for the
                    //fd -> thus it will be ready for
                    //read iff EPOLLIN & ev.events != 0
                    println!("Listener ready for connections");
                    for conn in self.listener.incoming() {
                        match conn {
                            Ok(stream) => {
                                let client_addr = stream.peer_addr()?;
                                println!("Accepted connection from {client_addr}");

                                self.poller.watch_socket(&stream)?;
                                self.clients
                                    .insert(stream.as_raw_fd(), client::Client::new(stream));

                                println!("Looper state {self:?}");
                            }
                            Err(err) => match err.kind() {
                                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => {
                                    continue 'event_loop;
                                }
                                _ => return Err(err),
                            },
                        }
                    }
                } else if self.clients.contains_key(&(descriptor as i32)) {
                    // println!("Got event from client: {ev:?}");

                    if (EPOLLIN as u32) & ev.events != 0 {
                        //we're guaranteed that descriptor is a valid key by the top level if and by
                        //this program being single threaded :D
                        let client = self.clients.get_mut(&(descriptor as i32)).unwrap();

                        let read_bytes = client.read_raw_cmd();
                        // println!("DEBUG: Read command {}", String::from_utf8(read_bytes.clone()).unwrap());

                        let cmd = RespType::try_from(read_bytes.as_slice())
                            .map(Command::from)
                            .unwrap_or_else(|err| Command::ErrorCmd {
                                msg: format!("Could not parse command, got error: {err}"),
                            });

                        match self.redis.handle_command(cmd, descriptor as i32) {
                            Ok(response) => {
                                println!("Sending response {response:?} to client {descriptor}");
                                client.send(response);
                                println!("Looper state {self:?}");
                            }
                            Err(err) => match err {
                                crate::redis::RedisError::Failure(_) => todo!(),
                                crate::redis::RedisError::WouldBlock => {
                                    /* do nothing, we come back at next iteration */
                                }
                            },
                        }
                    }

                    if (EPOLLOUT as u32) & ev.events != 0 {
                        let client = self.clients.get_mut(&(descriptor as i32)).unwrap();

                        if let Err(err) = client.flush() {
                            match err.kind() {
                                io::ErrorKind::WouldBlock => {
                                    println!("would block")
                                    /* do nothing we'll come back next time */
                                }
                                _ => {
                                    return Err(err);
                                }
                            }
                        }
                    }

                    //not exclusive cause it could be the case that the file desc is available for
                    //read operation even if EPOLLERR  | EPOLLHUP | EPOLLRDHUP have occurred (events
                    //are | together)
                    if ((EPOLLERR | EPOLLHUP | EPOLLRDHUP) as u32) & ev.events != 0 {
                        //the if condition guarantees that the key always is present in the clients
                        //map
                        println!("removing socket {descriptor}");
                        self.redis.remove_waiting(&(descriptor as i32));
                        let removed = self.clients.remove(&(descriptor as i32)).unwrap();
                        self.poller.remove_socket(removed.stream())?;
                        println!("Looper state {self:?}");
                    }
                }
            }
        }
    }
}
