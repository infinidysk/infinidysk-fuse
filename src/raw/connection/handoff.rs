//! The socket a mount helper (`fusermount3`, `mount_macfuse`) uses to hand back the FUSE
//! device descriptor.
//!
//! Every descriptor here is close-on-exec in this process. Only the helper's child process
//! clears the flag on its socket end, so no other child inherits the socket or the device.

use std::ffi::OsStr;
use std::io::{self, IoSliceMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::sys::socket::{self, AddressFamily, ControlMessageOwned, MsgFlags, SockFlag, SockType};

/// The environment variable that tells the helper which descriptor is its socket end.
const COMM_FD_ENV: &str = "_FUSE_COMMFD";

/// A connected socket pair: `(helper end, our end)`, both close-on-exec.
pub(super) fn comm_pair(ty: SockType) -> io::Result<(OwnedFd, OwnedFd)> {
    #[cfg(target_os = "linux")]
    let pair = socket::socketpair(AddressFamily::Unix, ty, None, SockFlag::SOCK_CLOEXEC)?;

    // ponytail: macOS has no SOCK_CLOEXEC, so another thread's fork+exec between these calls
    // can inherit the pair. Closing that window needs a process-wide fork lock.
    #[cfg(not(target_os = "linux"))]
    let pair = {
        let pair = socket::socketpair(AddressFamily::Unix, ty, None, SockFlag::empty())?;
        set_cloexec(&pair.0)?;
        set_cloexec(&pair.1)?;
        pair
    };

    Ok(pair)
}

/// A command for `program` that inherits `helper_end` and learns its number from
/// `_FUSE_COMMFD`.
pub(super) fn helper_command(program: impl AsRef<OsStr>, helper_end: &OwnedFd) -> Command {
    let fd = helper_end.as_raw_fd();
    let mut command = Command::new(program);
    command.env(COMM_FD_ENV, fd.to_string());

    // SAFETY: the closure runs in the forked child before exec and only calls `fcntl`, which
    // is async-signal-safe. `fd` stays open in the parent until spawn returns, because the
    // caller borrows `helper_end` across the spawn.
    unsafe {
        command.pre_exec(move || {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags == -1 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    command
}

/// Room for more descriptors than the one expected, so a faulty helper's extras are reported
/// and closed. Some kernels (macOS) install descriptors that do not fit without reporting them.
const MAX_FDS: usize = 8;

/// Blocks until the helper sends exactly one descriptor on `sock`, and returns it close-on-exec.
pub(super) fn recv_fd(sock: OwnedFd) -> io::Result<OwnedFd> {
    // The helper may send a byte of data with the descriptor; only the descriptor matters.
    let mut buf = [];
    let mut bufs = [IoSliceMut::new(&mut buf)];
    let mut cmsg_buf = nix::cmsg_space!([RawFd; MAX_FDS]);

    #[cfg(target_os = "linux")]
    let flags = MsgFlags::MSG_CMSG_CLOEXEC;
    // ponytail: macOS has no MSG_CMSG_CLOEXEC; the flag is set right after receipt below.
    #[cfg(not(target_os = "linux"))]
    let flags = MsgFlags::empty();

    let msg = socket::recvmsg::<()>(sock.as_raw_fd(), &mut bufs, Some(&mut cmsg_buf), flags)?;

    let mut received = Vec::new();
    // `cmsgs` fails when the control data was truncated (`MSG_CTRUNC`).
    for cmsg in msg.cmsgs()? {
        if let ControlMessageOwned::ScmRights(fds) = cmsg {
            // SAFETY: the kernel installed each descriptor of SCM_RIGHTS in this process just
            // now, and nothing else refers to it, so it is owned here exactly once.
            received.extend(
                fds.into_iter()
                    .map(|fd| unsafe { OwnedFd::from_raw_fd(fd) }),
            );
        }
    }

    // Any unexpected descriptor closes when `received` drops.
    let fd = match <[OwnedFd; 1]>::try_from(received) {
        Ok([fd]) => fd,
        Err(received) if received.is_empty() => {
            return Err(io::Error::other("mount helper sent no fuse fd"));
        }
        Err(received) => {
            return Err(io::Error::other(format!(
                "mount helper sent {} descriptors, expected one fuse fd",
                received.len()
            )));
        }
    };

    #[cfg(not(target_os = "linux"))]
    set_cloexec(&fd)?;

    Ok(fd)
}

#[cfg(not(target_os = "linux"))]
fn set_cloexec(fd: &OwnedFd) -> io::Result<()> {
    use nix::fcntl::{FcntlArg, FdFlag, fcntl};

    let flags = FdFlag::from_bits_retain(fcntl(fd, FcntlArg::F_GETFD)?);
    fcntl(fd, FcntlArg::F_SETFD(flags | FdFlag::FD_CLOEXEC))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{ErrorKind, IoSlice, Read};
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;
    use std::process::Stdio;

    use nix::fcntl::{FcntlArg, FdFlag, fcntl};
    use nix::sys::socket::ControlMessage;

    use std::sync::{Mutex, MutexGuard, PoisonError};

    use super::*;

    /// Serializes tests that create descriptors or spawn children. Without atomic
    /// close-on-exec (macOS), a child spawned by one test can inherit another test's descriptor.
    static SPAWN: Mutex<()> = Mutex::new(());

    fn serialize() -> MutexGuard<'static, ()> {
        SPAWN.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The socket type each platform's helper uses.
    #[cfg(target_os = "linux")]
    const TY: SockType = SockType::SeqPacket;
    #[cfg(not(target_os = "linux"))]
    const TY: SockType = SockType::Stream;

    fn is_cloexec(fd: impl AsFd) -> bool {
        let flags = fcntl(fd.as_fd(), FcntlArg::F_GETFD).expect("F_GETFD");
        FdFlag::from_bits_retain(flags).contains(FdFlag::FD_CLOEXEC)
    }

    fn send_fds(sock: &OwnedFd, fds: &[RawFd]) {
        let iov = [IoSlice::new(b"\0")];
        let cmsgs: &[ControlMessage] = if fds.is_empty() {
            &[]
        } else {
            &[ControlMessage::ScmRights(fds)]
        };
        socket::sendmsg::<()>(sock.as_raw_fd(), &iov, cmsgs, MsgFlags::empty(), None)
            .expect("sendmsg");
    }

    /// A pipe whose write end is sent through a helper pair and received back.
    fn received_pipe_writer() -> (UnixStream, OwnedFd) {
        let (reader, writer) = UnixStream::pair().expect("pair");
        let (helper, ours) = comm_pair(TY).expect("comm pair");
        send_fds(&helper, &[writer.as_raw_fd()]);
        drop(writer);
        (reader, recv_fd(ours).expect("receive fd"))
    }

    /// True once every copy of the peer of `reader` is closed.
    fn peer_closed(mut reader: &UnixStream) -> bool {
        reader.set_nonblocking(true).expect("nonblocking");
        match reader.read(&mut [0; 1]) {
            Ok(0) => true,
            Err(err) if err.kind() == ErrorKind::WouldBlock => false,
            other => panic!("unexpected read result {other:?}"),
        }
    }

    #[test]
    fn comm_pair_is_close_on_exec() {
        let _serial = serialize();
        let (helper, ours) = comm_pair(TY).expect("comm pair");
        assert!(is_cloexec(&helper));
        assert!(is_cloexec(&ours));
    }

    #[test]
    fn received_fd_is_close_on_exec() {
        let _serial = serialize();
        let (_reader, received) = received_pipe_writer();
        assert!(is_cloexec(&received));
    }

    #[test]
    fn unrelated_child_cannot_keep_received_fd() {
        let _serial = serialize();
        let (reader, received) = received_pipe_writer();
        // `cat` waits on stdin, so the child is alive while the check runs.
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .spawn()
            .expect("spawn cat");

        drop(received);
        let closed = peer_closed(&reader);
        drop(child.stdin.take());
        child.wait().expect("wait cat");

        assert!(closed, "the child inherited the received descriptor");
    }

    #[test]
    fn helper_inherits_only_its_end() {
        let _serial = serialize();
        let (helper, ours) = comm_pair(TY).expect("comm pair");
        let script = format!(
            "test -e /dev/fd/${COMM_FD_ENV} && ! test -e /dev/fd/{}",
            ours.as_raw_fd()
        );
        let status = helper_command("sh", &helper)
            .args(["-c", &script])
            .status()
            .expect("run sh");
        assert!(status.success(), "helper saw the wrong descriptors");
        assert!(
            is_cloexec(&helper),
            "parent's helper end stays close-on-exec"
        );
    }

    #[test]
    fn missing_fd_is_an_error() {
        let _serial = serialize();
        let (helper, ours) = comm_pair(TY).expect("comm pair");
        send_fds(&helper, &[]);
        assert!(recv_fd(ours).is_err());
    }

    #[test]
    fn closed_helper_is_an_error() {
        let _serial = serialize();
        let (helper, ours) = comm_pair(TY).expect("comm pair");
        drop(helper);
        assert!(recv_fd(ours).is_err());
    }

    #[test]
    fn extra_fds_are_closed_and_rejected() {
        let _serial = serialize();
        let (reader, writer) = UnixStream::pair().expect("pair");
        let (helper, ours) = comm_pair(TY).expect("comm pair");
        send_fds(&helper, &[writer.as_raw_fd(), writer.as_raw_fd()]);
        drop(writer);

        assert!(recv_fd(ours).is_err());
        assert!(peer_closed(&reader), "rejected descriptors were leaked");
    }
}
