use std::{
    io,
    collections::HashSet,
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd,
};

use libc::*;

#[derive(Debug)]
pub struct Poller {
    epoll_fd: i32,
    watched: HashSet<i32>,
}

impl Poller {
    const EPOLL_FLAGS_IGNORED: i32 = 0;

    //SAFETY if any syscall fails we return with the os error
    pub fn new(listener: &TcpListener) -> std::io::Result<Self> {
        let listener_fd = listener.as_raw_fd();

        let epoll_fd = unsafe {
            let epfd = epoll_create1(Self::EPOLL_FLAGS_IGNORED);
            if epfd < 0 {
                eprintln!("Error while opening epoll file descriptor");
                return Err(std::io::Error::last_os_error());
            }

            let mut event = epoll_event {
                events: EPOLLIN as u32,
                u64: listener_fd as u64,
            };

            let res = epoll_ctl(epfd, EPOLL_CTL_ADD, listener_fd, &mut event);
            if res == -1 {
                eprintln!("Error while registering listener file descriptor for polling");
                return Err(std::io::Error::last_os_error());
            }

            epfd
        };
        let mut watched = HashSet::new();
        watched.insert(listener_fd);

        Ok(Self { epoll_fd, watched })
    }

    pub fn watch_socket(&mut self, to_watch: &TcpStream) -> io::Result<()> {
        let to_watch_fd = to_watch.as_raw_fd();
        let inserted = self.watched.insert(to_watch_fd);
        if !inserted {
            return Err(io::Error::other("TcpStream is already being watched"));
        }

        let mut event = epoll_event {
            events: (EPOLLIN | EPOLLOUT | EPOLLRDHUP | EPOLLET) as u32, //EPOLLHUP and EPOLLERR are always
            //automatically reported
            u64: to_watch_fd as u64,
        };

        unsafe {
            let res = epoll_ctl(self.epoll_fd, EPOLL_CTL_ADD, to_watch_fd, &mut event);
            if res < 0 {
                let err = io::Error::last_os_error();
                return Err(err);
            }
        }

        Ok(())
    }

    pub fn remove_socket(&mut self, to_remove: &TcpStream) -> io::Result<()> {
        let to_remove_fd = to_remove.as_raw_fd();

        let removed = self.watched.remove(&to_remove_fd);
        if !removed {
            return Err(io::Error::other(
                "Trying to remove non currently watched file",
            ));
        }

        let mut ignored = epoll_event {
            events: 0u32,
            u64: to_remove_fd as u64,
        };

        unsafe {
            let res = epoll_ctl(
                self.epoll_fd,
                EPOLL_CTL_DEL,
                to_remove_fd,
                &mut ignored,
            );
            if res < 0 {
                let err = io::Error::last_os_error();
                return Err(err);
            }
        }

        Ok(())
    }

    //SAFETY the epoll file descriptor is encapsulated by the Poller struct and the events buffer is created inside this method and not taken as an argument
    pub fn poll(&self) -> io::Result<Vec<epoll_event>> {
        let mut events = vec![epoll_event { events: 0, u64: 0 }; 128];

        let n = unsafe {
            let n = epoll_wait(
                self.epoll_fd,
                events.as_mut_ptr(),
                events.len() as i32,
                -1,
            );

            if n < 0 {
                println!("Error while waiting for epoll events");
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    return Ok(vec![]);
                } else {
                    return Err(err);
                }
            }

            n
        };

        Ok(events[..n as usize].to_vec())
    }
}
