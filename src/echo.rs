use std::os::fd::RawFd;
use io_uring::{cqueue, IoUring, opcode, squeue};
use io_uring::types::Fd;
use log::debug;

pub struct Connection {
    fd: Fd,
    state: ConnectionState,
    data_off: u32,
    data_len: u32,
    buf: [u8; 4096],
}

enum ConnectionState {
    Reading,
    Writing,
    Closing,
}

impl Connection {
    pub fn new(fd: RawFd) -> Connection {
        Connection {
            fd: Fd(fd),
            state: ConnectionState::Reading,
            data_off: 0,
            data_len: 0,
            buf: [0u8; 4096],
        }
    }

    pub fn register(&mut self, ring: &IoUring) {
        debug!("registering new connection on fd {}", self.fd.0);
        let sqe = self.next_sqe();
        unsafe { ring.submission_shared().push(&sqe).unwrap(); }
    }

    pub fn process_cqe(&mut self, cqe: &cqueue::Entry) -> Option<squeue::Entry> {
        match &self.state {
            ConnectionState::Reading => {
                if cqe.result() < 1 {
                    self.state = ConnectionState::Closing;
                } else {
                    self.data_len += cqe.result() as u32;
                    self.state = ConnectionState::Writing;
                }
                Some(self.next_sqe())
            },
            ConnectionState::Writing => {
                if cqe.result() < 1 {
                    self.state = ConnectionState::Closing;
                } else {
                    self.data_off += cqe.result() as u32;
                    self.data_len -= cqe.result() as u32;
                    if self.data_len == 0 {
                        self.data_off = 0;
                        self.state = ConnectionState::Reading;
                    }
                }
                Some(self.next_sqe())
            },
            ConnectionState::Closing => None
        }
    }

    fn next_sqe(&mut self) -> squeue::Entry {
        match &self.state {
            ConnectionState::Reading => {
                let read_buf_ptr = unsafe { self.buf.as_mut_ptr().add((self.data_off + self.data_len) as usize) };
                opcode::Read::new(
                    self.fd,
                    read_buf_ptr,
                    self.buf.len() as u32 - (self.data_off + self.data_len),
                )
                    .build()
                    .user_data(self.as_u64())
            }
            ConnectionState::Writing => {
                let write_buf_ptr = unsafe { self.buf.as_mut_ptr().add(self.data_off as usize) };
                opcode::Write::new(
                    self.fd,
                    write_buf_ptr,
                    self.data_len
                )
                    .build()
                    .user_data(self.as_u64())
            },
            ConnectionState::Closing => opcode::Close::new(self.fd)
                .build()
                .user_data(self.as_u64())
        }
    }

    fn as_u64(&self) -> u64 {
        let conn_ptr: *const Connection = self;
        conn_ptr as u64
    }
}
