use std::io;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(all(not(feature = "tokio-runtime"), feature = "async-io-runtime"))]
pub use async_io::FuseConnection;
#[cfg(all(not(feature = "async-io-runtime"), feature = "tokio-runtime"))]
pub use tokio::FuseConnection;

#[cfg(test)]
pub(crate) use memory::KernelSide;

#[cfg(feature = "async-io-runtime")]
mod async_io;
#[cfg(test)]
mod memory;
#[cfg(feature = "tokio-runtime")]
mod tokio;

pub(crate) type CompleteIoResult<T, U> = (T, io::Result<U>);

/// Blocking reads that may still run. A read counts from before it is queued until its
/// closure is dropped, so a dropped read future cannot hide it.
#[derive(Debug, Default)]
pub(crate) struct BlockingReads {
    state: Mutex<ReadsState>,
    exited: async_notify::Notify,
}

#[derive(Debug, Default)]
struct ReadsState {
    running: usize,
    closed: bool,
}

impl BlockingReads {
    fn state(&self) -> MutexGuard<'_, ReadsState> {
        // The state stays consistent across a panic: every update is a single field write.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a read, or returns `None` once closed so no read starts after shutdown.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn start(self: &Arc<Self>) -> Option<ReadGuard> {
        let mut state = self.state();
        if state.closed {
            return None;
        }
        state.running += 1;
        Some(ReadGuard(self.clone()))
    }

    pub(crate) fn close(&self) {
        self.state().closed = true;
    }

    pub(crate) fn idle(&self) -> bool {
        self.state().running == 0
    }

    /// Completes once no read runs. Only one caller may wait at a time.
    pub(crate) async fn exited(&self) {
        while !self.idle() {
            self.exited.notified().await;
        }
    }
}

/// Owned by a blocking read closure; dropping it ends the read's registration.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
struct ReadGuard(Arc<BlockingReads>);

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for ReadGuard {
    fn drop(&mut self) {
        let idle = {
            let mut state = self.0.state();
            state.running -= 1;
            state.running == 0
        };
        if idle {
            self.0.exited.notify();
        }
    }
}

/// Error a blocking read returns once its connection is shut down; dispatch treats it as the end.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn shut_down_error() -> io::Error {
    io::Error::from_raw_os_error(libc::ENODEV)
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(crate) mod test_util {
    use std::fs::File;
    use std::future::Future;
    use std::io::{ErrorKind, Read};
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::pin::pin;
    use std::sync::Arc;
    use std::time::Duration;

    use futures_util::future::{self, Either};

    /// A connected socket pair: one end as the connection's `File`, and its peer.
    pub(crate) fn pair() -> (File, UnixStream) {
        let (ours, peer) = UnixStream::pair().expect("socketpair");
        (File::from(OwnedFd::from(ours)), peer)
    }

    /// The bytes waiting on `file`, read without blocking.
    pub(crate) fn unread(file: File) -> Vec<u8> {
        let stream = UnixStream::from(OwnedFd::from(file));
        stream.set_nonblocking(true).expect("set nonblocking");
        let mut buf = vec![0; 64];
        match (&stream).read(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                buf
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => Vec::new(),
            Err(err) => panic!("read unread bytes: {err}"),
        }
    }

    /// The output of `fut`, or `None` after `limit`. The timer is a thread, outside the runtime.
    pub(crate) async fn within<F: Future>(limit: Duration, fut: F) -> Option<F::Output> {
        let expired = Arc::new(async_notify::Notify::new());
        let notify = expired.clone();
        std::thread::spawn(move || {
            std::thread::sleep(limit);
            notify.notify();
        });

        match future::select(pin!(fut), pin!(expired.notified())).await {
            Either::Left((output, _)) => Some(output),
            Either::Right(_) => None,
        }
    }
}
