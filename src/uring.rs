use std::os::fd::{AsRawFd, RawFd};
use io_uring::{cqueue, IoUring, opcode, squeue, types};
use crate::uring::UringMsg::NewConnection;

const RING_MSG_NEW_CONN: i32 = i32::MIN + 1;

pub enum UringMsg {
    NewConnection(RawFd)
}

impl UringMsg {
    pub fn try_from_cqe(cqe: &cqueue::Entry) -> Option<Self> {
        match cqe.result() {
            RING_MSG_NEW_CONN => Some(NewConnection(cqe.user_data() as i32)),
            _ => None
        }
    }
}

pub struct UringHandle {
    ring_fd: RawFd
}

impl UringHandle {
    pub fn new(uring: &IoUring) -> Self {
        UringHandle {
            ring_fd: uring.as_raw_fd()
        }
    }

    /// Returns an SQE to send the given msg to this uring
    pub fn create_send_sqe(&self, msg: UringMsg) -> squeue::Entry {
        match msg { UringMsg::NewConnection(conn_fd) => {
            opcode::MsgRingData::new(types::Fd { 0: self.ring_fd }, RING_MSG_NEW_CONN, conn_fd as u64, None).build()
        } }
    }
}
