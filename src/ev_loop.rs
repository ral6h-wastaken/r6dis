use std::{
    collections::HashMap,
    io::{self},
    net::TcpListener,
    os::fd::AsRawFd,
    time
};

mod client;

use libc::{EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLOUT, EPOLLRDHUP};

use crate::{command::Command, poll::Poller, redis::Redis, resp::RespType};

#[derive(Debug)]
pub struct EventLoop {
    redis: Redis,
    waiting: HashMap<i32, Callback>,
    listener: TcpListener,
    clients: HashMap<i32, client::Client>,
    poller: Poller,
}

#[derive(Debug)]
struct Callback {
    expiry: Option<time::Instant>,
}

impl EventLoop {
    pub fn new(listener: TcpListener, poller: Poller) -> Self {
        Self {
            redis: Redis::new(),
            listener,
            poller,
            clients: HashMap::new(),
            waiting: HashMap::new(),
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        println!("Starting main event loop. Listening for connections at port 6379");
        let listener_fd = self.listener.as_raw_fd() as u64;

        'event_loop: loop {
            // println!("Looper state {self:?}");

            //TODO [LS]: try to wake waiting clients
            let mut to_remove = vec![];

            for cl in self.waiting.keys() {
                let cb = self.waiting.get(cl);
                if let Some(ttl) = cb.and_then(|cb| cb.expiry)
                    && ttl <= time::Instant::now()
                {
                    let client = self.clients.get_mut(cl).unwrap();

                    client.send(RespType::SimpleString {
                        content: "Timeout!".into(),
                    });

                    to_remove.push(*cl);
                }
            }

            for tr in to_remove {
                self.waiting.remove(&tr);
            }

            let events = self.poller.poll(1_000)?;

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
                        println!("Socket {descriptor} available for read");

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

                        match self.redis.handle_command(cmd) {
                            Ok(response) => client.send(response),
                            Err(err) => match err {
                                crate::redis::RedisError::Failure(_) => todo!(),
                                crate::redis::RedisError::WouldBlock { timeout } => {
                                    self.waiting.insert(descriptor as i32, Callback {
                                        expiry: timeout.map(|t|/*TODO [LS]: fix*/ time::Instant::now() + t),
                                    });
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
                        self.waiting.remove(&(descriptor as i32));
                        let removed = self.clients.remove(&(descriptor as i32)).unwrap();
                        self.poller.remove_socket(removed.stream())?;
                    }
                }
            }
        }
    }
}
