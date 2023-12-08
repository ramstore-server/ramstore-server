use std::io::ErrorKind;
pub use std::os::fd::OwnedFd;
use std::os::unix::io::AsRawFd;
use std::thread;
use io_uring::{IoUring, opcode, types};
use crate::uring::{UringHandle, UringMsg};

mod uring;
mod echo;

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let mut accept_ring = IoUring::new(8)?;
    let mut echo_ring = IoUring::new(256)?;
    let accept_handle = UringHandle::new(&accept_ring);
    let echo_handle = UringHandle::new(&echo_ring);

    let listener = std::net::TcpListener::bind("127.0.0.1:6379")?;

    thread::scope(|s| {
        s.spawn(|| {
            accept_loop(&mut accept_ring, OwnedFd::from(listener), &echo_handle);
        });
        s.spawn(|| {
            echo_loop(&mut echo_ring);
        });
    });

    Ok(())
}

fn accept_loop(accept_ring: &mut IoUring, redis_listen_socket: OwnedFd, echo_handle: &UringHandle) {
    let accept_e = opcode::AcceptMulti::new(
        types::Fd { 0: redis_listen_socket.as_raw_fd() }
    ).build();
    unsafe {
        accept_ring.submission().push(&accept_e).unwrap();
        accept_ring.submission().sync();
    }

    loop {
        match accept_ring.submit_and_wait(1) {
            Ok(_) => (),
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => (),
        }

        unsafe {
            loop {
                let cq = accept_ring.completion_shared();
                if cq.is_empty() {
                    break;
                }

                for cqe in cq {
                    let conn_fd = cqe.result();
                    let send_sqe = echo_handle.create_send_sqe(UringMsg::NewConnection(conn_fd));
                    accept_ring.submission_shared().push(&send_sqe).unwrap();
                }
            }
        }
    }
}

fn echo_loop(echo_ring: &mut IoUring) {
    loop {
        match echo_ring.submit_and_wait(1) {
            Ok(_) => (),
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(_) => (),
        }

        loop {
            unsafe {
                let cq = echo_ring.completion_shared();
                if cq.is_empty() {
                    break;
                }

                for cqe in cq {
                    let handled = match UringMsg::try_from_cqe(&cqe) {
                        Some(UringMsg::NewConnection(fd)) => {
                            let conn = Box::leak(Box::new(echo::Connection::new(fd)));
                            conn.register(echo_ring);
                            true
                        }
                        None => false
                    };

                    if handled {
                        continue;
                    }

                    let mut conn = Box::from_raw(cqe.user_data() as *mut echo::Connection);
                    match conn.process_cqe(&cqe) {
                        Some(sqe) => {
                            echo_ring.submission_shared().push(&sqe).unwrap();
                            Box::leak(conn);
                        }
                        None => (), // let the connection drop, we are done
                    }
                }
            }
        }
    }
}
