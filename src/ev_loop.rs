use std::{
    collections::HashMap,
    io::{self, Read as _, Write as _},
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd,
};

use libc::{EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLOUT, EPOLLRDHUP};

use crate::poll::Poller;

#[derive(Debug)]
struct Client {
    stream: TcpStream,
    buffer: Vec<u8>,
}

#[derive(Debug)]
pub struct EventLoop {
    listener: TcpListener,
    clients: HashMap<i32, Client>,
    poller: Poller,
}

impl EventLoop {
    pub fn new(listener: TcpListener, poller: Poller) -> Self {
        Self {
            listener,
            poller,
            clients: HashMap::new(),
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        println!("Starting main event loop. Listening for connections at port 6379");
        let listener_fd = self.listener.as_raw_fd() as u64;

        'event_loop: loop {
            println!("Looper state {self:?}");
            let events = self.poller.poll()?;

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
                                self.clients.insert(
                                    stream.as_raw_fd(),
                                    Client {
                                        stream,
                                        buffer: vec![],
                                    },
                                );
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

                        let mut buf = [0u8; 512];
                        //we're guaranteed that descriptor is a valid key by the top level if and by
                        //this program being single threaded :D
                        let client = self.clients.get_mut(&(descriptor as i32)).unwrap();

                        let mut socket = &client.stream;
                        let buffer = &mut client.buffer;

                        let to_echo = match socket.read(&mut buf) {
                            Err(err) => {
                                let errmsg = format!("Could not read from socket, got error {err}");
                                errmsg.into_bytes()
                            }
                            Ok(read) => buf[..read].to_vec(),
                        };

                        let msg = String::from_utf8(to_echo.clone()).unwrap();
                        println!("received message {msg}");

                        b"+PONG\r\n".iter().for_each(|b| buffer.push(b.clone()));
                    }

                    if (EPOLLOUT as u32) & ev.events != 0 {
                        let c = self.clients.get_mut(&(descriptor as i32)).unwrap();
                        if !c.buffer.is_empty() {
                            println!("Socket {descriptor} available for write");

                            if let Err(err) = c.stream.write_all(&c.buffer) {
                                match err.kind() {
                                    io::ErrorKind::WouldBlock => {
                                        /* do nothing we'll come back next time */
                                    }
                                    _ => {
                                        return Err(err);
                                    }
                                }
                            }
                            
                            c.buffer.clear();
                        }
                    }

                    //not exclusive cause it could be the case that the file desc is available for
                    //read operation even if EPOLLERR  | EPOLLHUP | EPOLLRDHUP have occurred (events
                    //are | together)
                    if ((EPOLLERR | EPOLLHUP | EPOLLRDHUP) as u32) & ev.events != 0 {
                        //the if condition guarantees that the key always is present in the clients
                        //map
                        println!("removing socket {descriptor}");
                        let removed = self.clients.remove(&(descriptor as i32)).unwrap();
                        self.poller.remove_socket(&removed.stream)?;
                    }
                }
            }
        }
    }
}
