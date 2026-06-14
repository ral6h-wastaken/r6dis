#![allow(unused_imports)]
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd as _,
    str::FromStr as _,
};

mod ev_loop;
mod poll;
mod resp;
mod command;

use crate::poll::Poller;

fn main() -> std::io::Result<()> {
    const PORT: u32 = 6379;

    let listener = TcpListener::bind(format!("127.0.0.1:{PORT}").as_str())
        .expect(format!("Error while binding to port {PORT}").as_str());

    listener.set_nonblocking(true).map_err(|err| {
        eprintln!("Could not set listener to non blocking mode");
        err
    })?;

    let poller = Poller::new(&listener)?;
    let mut looper = crate::ev_loop::EventLoop::new(listener, poller);

    looper.run()
}
