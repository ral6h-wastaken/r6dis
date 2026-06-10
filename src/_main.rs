/// Single-threaded echo server using Linux epoll via libc.
///
/// Handles unlimited concurrent connections in one thread:
///   - Accepts new connections without blocking
///   - Reads and echoes data back as it arrives
///   - Keeps connections alive until the client disconnects
///   - Never spawns a thread per connection

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};

use libc::{
    epoll_create1, epoll_ctl, epoll_event, epoll_wait, EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLRDHUP,
    EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL,
};

// ─── helpers ────────────────────────────────────────────────────────────────

/// Register `fd` with the epoll instance `epfd`.
/// `events` is a bitmask of EPOLLIN, EPOLLOUT, EPOLLET, etc.
/// `token` is stored in epoll_event.u64 — we use it to look up which fd fired.
fn epoll_add(epfd: RawFd, fd: RawFd, token: u64, events: u32) -> io::Result<()> {
    let mut ev = epoll_event {
        events,
        u64: token,
    };
    let ret = unsafe { epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &mut ev) };
    if ret == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Remove `fd` from the epoll instance `epfd`.
/// Must be called before closing the fd, otherwise epoll keeps a stale entry.
fn epoll_remove(epfd: RawFd, fd: RawFd) -> io::Result<()> {
    // Linux 2.6.9+ ignores the event pointer on DEL, but we pass one anyway
    // for compatibility with older kernels.
    let mut ev = epoll_event { events: 0, u64: 0 };
    let ret = unsafe { epoll_ctl(epfd, EPOLL_CTL_DEL, fd, &mut ev) };
    if ret == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// ─── event loop ─────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    // 1. Bind the listener socket.
    let listener = TcpListener::bind("127.0.0.1:7878")?;
    listener.set_nonblocking(true)?;
    println!("Listening on 127.0.0.1:7878 …");

    // 2. Create the epoll instance.
    //    EPOLL_CLOEXEC closes the epoll fd automatically on exec().
    let epfd = unsafe { epoll_create1(EPOLL_CLOEXEC) };
    if epfd == -1 {
        return Err(io::Error::last_os_error());
    }

    // 3. Register the listener fd.
    //    We use the raw fd itself as the token — simple and unique.
    //    EPOLLIN fires when a new TCP connection is waiting to be accept()ed.
    let listener_fd = listener.as_raw_fd();
    epoll_add(epfd, listener_fd, listener_fd as u64, EPOLLIN as u32)?;

    // Map from raw fd → TcpStream so we can read/write after epoll wakes us.
    // We store the stream here so Rust keeps the fd alive (dropping TcpStream
    // would close the fd and confuse epoll).
    let mut connections: HashMap<RawFd, TcpStream> = HashMap::new();

    // Scratch buffer — reused every iteration to avoid per-event allocations.
    let mut buf = [0u8; 4096];

    // Pre-allocate the events array that epoll_wait writes into.
    // Size = max events to process per epoll_wait call (tune as needed).
    let mut events = vec![epoll_event { events: 0, u64: 0 }; 128];

    // ── main loop ────────────────────────────────────────────────────────────
    loop {
        // epoll_wait blocks until at least one fd is ready, then fills
        // `events[0..n]` with descriptors that need attention.
        // timeout = -1 -> block forever (use 0 for polling, >0 for a deadline).
        let n = unsafe {
            epoll_wait(
                epfd,
                events.as_mut_ptr(),
                events.len() as i32,
                -1, // timeout_ms: -1 = block until an event arrives
            )
        };
        if n == -1 {
            let err = io::Error::last_os_error();
            // EINTR just means a signal interrupted us — restart the loop.
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }

        // Iterate only over the events that actually fired.
        for ev in &events[..n as usize] {
            let token = ev.u64 as RawFd;
            let ready = ev.events;

            if token == listener_fd {
                // ── new connection ───────────────────────────────────────────
                // The listener is ready: accept as many connections as possible
                // (loop until EAGAIN, because we're in non-blocking mode).
                loop {
                    match listener.accept() {
                        Ok((stream, addr)) => {
                            println!("[+] Connection from {addr}");
                            let fd = stream.as_raw_fd();
                            stream.set_nonblocking(true)?;

                            // Watch for: data to read, peer hung up, errors.
                            // EPOLLRDHUP tells us when the remote end closed.
                            let interests =
                                (EPOLLIN | EPOLLRDHUP | EPOLLERR | EPOLLHUP) as u32;
                            epoll_add(epfd, fd, fd as u64, interests)?;

                            connections.insert(fd, stream);
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            // No more pending connections right now — done.
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
            } else {
                // ── existing connection ──────────────────────────────────────

                // Detect disconnect: peer closed their side, or error.
                let disconnected = (ready & (EPOLLRDHUP | EPOLLHUP | EPOLLERR) as u32) != 0;

                if !disconnected {
                    // Data is available — read a chunk and echo it back.
                    // We loop until EAGAIN to drain the socket buffer fully,
                    // which is important if you later switch to edge-triggered
                    // (EPOLLET) mode where epoll fires only once per edge.
                    if let Some(stream) = connections.get_mut(&token) {
                        let mut closed = false;
                        loop {
                            match stream.read(&mut buf) {
                                Ok(0) => {
                                    // read() returning 0 means EOF (clean close).
                                    closed = true;
                                    break;
                                }
                                Ok(n) => {
                                    // Echo the received bytes back verbatim.
                                    // In a real server you'd queue partial writes
                                    // and register EPOLLOUT if write_all blocks.
                                    if let Err(e) = stream.write_all(&buf[..n]) {
                                        eprintln!("Write error on fd {token}: {e}");
                                        closed = true;
                                        break;
                                    }
                                }
                                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                    // Socket buffer is empty — come back next time.
                                    break;
                                }
                                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                                    // Signal interrupted the syscall — retry.
                                    continue;
                                }
                                Err(e) => {
                                    eprintln!("Read error on fd {token}: {e}");
                                    closed = true;
                                    break;
                                }
                            }
                        }
                        if closed {
                            // Trigger the cleanup path below.
                            // We fall through by removing from `connections` after
                            // the borrow ends — see the block below.
                        } else {
                            continue; // still open, nothing more to do
                        }
                    }
                }

                // ── cleanup ──────────────────────────────────────────────────
                // Remove from epoll BEFORE dropping (closing) the stream.
                // Dropping TcpStream closes the fd; if epoll still holds a
                // reference to a closed fd you get EBADF on the next wait.
                if let Err(e) = epoll_remove(epfd, token) {
                    eprintln!("epoll_remove error for fd {token}: {e}");
                }
                connections.remove(&token);
                // TcpStream is dropped here → fd is closed → OS frees resources.
                println!("[-] Closed connection fd {token}");
            }
        }
    }
}
