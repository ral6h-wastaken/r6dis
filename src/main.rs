use std::
    net::TcpListener
;

mod ev_loop;
mod poll;
mod resp;
mod command;
mod redis;

use crate::{ev_loop::EventLoop, poll::Poller};

fn main() -> std::io::Result<()> {
    const PORT: u32 = 6379;

    let listener = TcpListener::bind(format!("127.0.0.1:{PORT}").as_str())
        .unwrap_or_else(|_| panic!("Error while binding to port {PORT}"));

    listener.set_nonblocking(true).inspect_err(|_| {
        eprintln!("Could not set listener to non blocking mode");
    })?;

    let poller = Poller::new(&listener)?;
    let mut looper = EventLoop::new(listener, poller);

    looper.run()
}
