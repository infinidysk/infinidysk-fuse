//! The socket a mount helper (`fusermount3`, `mount_macfuse`) uses to hand back the FUSE
//! device descriptor.
//!
//! Every descriptor here is close-on-exec in this process. Only the helper's child process
//! clears the flag on its socket end, so no other child inherits the socket or the device.
//!
//! On Linux the flag is set atomically, when the socket is created and when the descriptor is
//! received. macOS has no atomic form, so the flag is set right after each call; a child that
//! another thread forks in that short window can still inherit the descriptor.

#[cfg(target_os = "linux")]
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

/// Spawns `command` with `spawn` so that the child inherits `helper_end` and learns its number
/// from `_FUSE_COMMFD`. `helper_end` is closed in this process once `spawn` returns.
pub(super) fn spawn_helper<T>(
    mut command: Command,
    helper_end: OwnedFd,
    spawn: impl FnOnce(Command) -> io::Result<T>,
) -> io::Result<T> {
    let fd = helper_end.as_raw_fd();
    command.env(COMM_FD_ENV, fd.to_string());

    // SAFETY: the closure runs in the forked child before exec and only calls `fcntl`, which
    // is async-signal-safe. `fd` refers to `helper_end`, which this function owns until
    // `spawn` returns, so the number cannot be reused for another descriptor meanwhile.
    unsafe {
        command.pre_exec(move || {
            let flags = libc::fcntl(fd, libc::F_GETFD);
            if flags == -1 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let spawned = spawn(command);
    drop(helper_end);
    spawned
}

/// Unmounts `mount_path` after `fusermount3` mounted it but the descriptor handoff failed.
/// Without this, closing the device aborts the connection but leaves a dead mount behind.
#[cfg(target_os = "linux")]
pub(super) fn unmount_after_failed_handoff(fusermount: &std::path::Path, mount_path: &OsStr) {
    let status = Command::new(fusermount)
        .args([
            OsStr::new("-u"),
            OsStr::new("-z"),
            OsStr::new("--"),
            mount_path,
        ])
        .status();

    match status {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::warn!(?status, "unmount after a failed fd handoff failed"),
        Err(error) => tracing::warn!(%error, "unmount after a failed fd handoff failed"),
    }
}

/// Room for every descriptor one message can carry on Linux (`SCM_MAX_FD`), so the control data
/// is never truncated and a faulty helper's extras are all owned here and closed. When the
/// control data is truncated, Linux installs descriptors that nix does not return.
const MAX_FDS: usize = 253;

/// Blocks until the helper sends exactly one descriptor on `sock`, and returns it close-on-exec.
pub(super) fn recv_fd(sock: OwnedFd) -> io::Result<OwnedFd> {
    // The helper sends one byte with the descriptor. Receiving into a non-empty buffer makes
    // a stream socket (macOS) wait for that byte instead of returning at once; it returns empty
    // when the helper exits without sending.
    let mut buf = [0; 1];
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
        let mut command = Command::new("sh");
        command.args(["-c", &script]);
        let mut child =
            spawn_helper(command, helper, |mut command| command.spawn()).expect("run sh");
        assert!(
            child.wait().expect("wait sh").success(),
            "helper saw the wrong descriptors"
        );

        // The helper exited and the parent's copy was closed by spawn_helper.
        let ours = UnixStream::from(ours);
        assert!(peer_closed(&ours), "spawn_helper kept the helper end open");
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
    fn receive_waits_for_a_late_send() {
        let _serial = serialize();
        let (reader, writer) = UnixStream::pair().expect("pair");
        let (helper, ours) = comm_pair(TY).expect("comm pair");
        let sender = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            send_fds(&helper, &[writer.as_raw_fd()]);
        });

        let received = recv_fd(ours).expect("receive the late fd");
        sender.join().expect("sender thread");
        drop(received);
        assert!(peer_closed(&reader));
    }

    #[test]
    fn many_extra_fds_are_closed_and_rejected() {
        let _serial = serialize();
        let (reader, writer) = UnixStream::pair().expect("pair");
        let (helper, ours) = comm_pair(TY).expect("comm pair");
        send_fds(&helper, &[writer.as_raw_fd(); 64]);
        drop(writer);

        assert!(recv_fd(ours).is_err());
        assert!(peer_closed(&reader), "rejected descriptors were leaked");
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
