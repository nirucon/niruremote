//! Blocking event sources for the long-running client and agent processes.
//!
//! Every wait in a session is a blocking call on its own thread that sends a
//! message when something happens: a child exits, a signal arrives, a stream
//! closes. The main thread blocks on the channel. Nothing polls on a timer.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::Sender;

#[derive(Debug)]
pub enum Event {
    /// A watched child process exited (it is not reaped; its owner does that).
    Exited(&'static str),
    /// SIGINT, SIGTERM or SIGHUP.
    Signal,
    /// A one-shot listener accepted the viewer connection.
    Accepted(Result<std::os::fd::OwnedFd, String>),
    /// A relay direction reached end-of-file or an error.
    Closed(&'static str),
}

/// Send `Event::Exited(tag)` when `pid` exits, without reaping it.
pub fn watch_exit(pid: u32, tag: &'static str, sender: Sender<Event>) {
    std::thread::spawn(move || {
        loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOWAIT,
                )
            };
            if result == 0 {
                let _ = sender.send(Event::Exited(tag));
                return;
            }
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                // ECHILD: already reaped by its owner, which knows it exited.
                let _ = sender.send(Event::Exited(tag));
                return;
            }
        }
    });
}

static SIGNAL_PIPE: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(_: libc::c_int) {
    let fd = SIGNAL_PIPE.load(Ordering::Relaxed);
    if fd >= 0 {
        let byte = 1u8;
        // write(2) is async-signal-safe; a full pipe just drops the byte.
        unsafe {
            libc::write(fd, (&byte as *const u8).cast(), 1);
        }
    }
}

/// Deliver SIGINT/SIGTERM/SIGHUP as `Event::Signal` through a self-pipe.
pub fn watch_signals(sender: Sender<Event>) -> Result<(), String> {
    let mut fds = [0 as libc::c_int; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return Err(format!("signal pipe: {}", std::io::Error::last_os_error()));
    }
    let (read_fd, write_fd) = (fds[0], fds[1]);
    // The reader blocks; only the writer end must be non-blocking.
    unsafe {
        let flags = libc::fcntl(read_fd, libc::F_GETFL);
        libc::fcntl(read_fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
    }
    SIGNAL_PIPE.store(write_fd, Ordering::SeqCst);
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        unsafe {
            libc::signal(signal, on_signal as *const () as libc::sighandler_t);
        }
    }
    std::thread::spawn(move || {
        let mut byte = 0u8;
        loop {
            let count = unsafe { libc::read(read_fd, (&mut byte as *mut u8).cast(), 1) };
            if count == 1 {
                if sender.send(Event::Signal).is_err() {
                    return;
                }
            } else if count == 0
                || std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR)
            {
                return;
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn exit_is_reported_without_reaping() {
        let mut child = Command::new("sh").arg("-c").arg("exit 3").spawn().unwrap();
        let (sender, receiver) = mpsc::channel();
        watch_exit(child.id(), "child", sender);
        match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Exited(tag) => assert_eq!(tag, "child"),
            other => panic!("unexpected {other:?}"),
        }
        // The owner can still reap it and read the real status.
        assert_eq!(child.wait().unwrap().code(), Some(3));
    }
}
