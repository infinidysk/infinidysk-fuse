#[cfg(target_os = "macos")]
use std::env;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::io::Write;
use std::io::{IoSlice, IoSliceMut};
use std::ops::DerefMut;
use std::os::fd::AsFd;
use std::os::fd::BorrowedFd;
#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "freebsd",
    target_os = "macos"
))]
use std::os::fd::OwnedFd;
#[cfg(target_os = "macos")]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "macos"
))]
use std::os::unix::io::RawFd;
use std::pin::pin;
use std::sync::Arc;
#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "macos"
))]
use std::{ffi::OsString, path::Path};

#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "freebsd"
))]
use async_io::Async;
use async_lock::Mutex;
use async_notify::Notify;
#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "macos"
))]
use async_process::Command;
use futures_util::{FutureExt, select};
#[cfg(target_os = "macos")]
use futures_util::{join, try_join};
#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "macos"
))]
use nix::sys::socket::{self, AddressFamily, ControlMessageOwned, MsgFlags, SockFlag, SockType};
#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "freebsd",
    target_os = "macos"
))]
use nix::sys::uio;
#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "macos"
))]
use tracing::debug;

#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "macos"
))]
use crate::MountOptions;
#[cfg(all(target_os = "linux", feature = "unprivileged"))]
use crate::find_fusermount3;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::raw::connection::shut_down_error;
use crate::raw::connection::{BlockingReads, CompleteIoResult};
#[derive(Debug)]
pub struct FuseConnection {
    unmount_notify: Arc<Notify>,
    reads: Arc<BlockingReads>,
    mode: ConnectionMode,
}

impl FuseConnection {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub fn new(unmount_notify: Arc<Notify>) -> io::Result<Self> {
        #[cfg(target_os = "freebsd")]
        {
            let connection = NonBlockFuseConnection::new()?;

            Ok(Self {
                unmount_notify,
                reads: Arc::default(),
                mode: ConnectionMode::NonBlock(connection),
            })
        }

        #[cfg(target_os = "linux")]
        {
            let connection = BlockFuseConnection::new()?;

            Ok(Self {
                unmount_notify,
                reads: Arc::default(),
                mode: ConnectionMode::Block(connection),
            })
        }
    }

    #[cfg(all(target_os = "linux", feature = "unprivileged"))]
    pub async fn new_with_unprivileged(
        mount_options: MountOptions,
        mount_path: impl AsRef<Path>,
        unmount_notify: Arc<Notify>,
    ) -> io::Result<Self> {
        let connection =
            NonBlockFuseConnection::new_with_unprivileged(mount_options, mount_path).await?;

        Ok(Self {
            unmount_notify,
            reads: Arc::default(),
            mode: ConnectionMode::NonBlock(connection),
        })
    }

    #[cfg(target_os = "macos")]
    pub async fn new_with_unprivileged(
        mount_options: MountOptions,
        mount_path: impl AsRef<Path>,
        unmount_notify: Arc<Notify>,
    ) -> io::Result<Self> {
        let connection =
            BlockFuseConnection::new_with_unprivileged(mount_options, mount_path).await?;

        Ok(Self {
            unmount_notify,
            reads: Arc::default(),
            mode: ConnectionMode::Block(connection),
        })
    }

    #[cfg(test)]
    pub(crate) fn memory() -> (Self, super::KernelSide) {
        let (connection, kernel) = super::memory::MemoryConnection::new();

        (
            Self {
                unmount_notify: Arc::new(Notify::new()),
                reads: Arc::default(),
                mode: ConnectionMode::Memory(connection),
            },
            kernel,
        )
    }

    /// The current or next `read_vectored` returns `None`, which ends dispatch.
    pub(crate) fn shut_down(&self) {
        self.reads.close();
        self.unmount_notify.notify();
    }

    /// The blocking reads of this connection, which can outlive their `read_vectored` futures.
    pub(crate) fn blocking_reads(&self) -> Arc<BlockingReads> {
        self.reads.clone()
    }

    pub async fn read_vectored<T: DerefMut<Target = [u8]> + Send + 'static>(
        &self,
        header_buf: Vec<u8>,
        data_buf: T,
    ) -> Option<CompleteIoResult<(Vec<u8>, T), usize>> {
        let mut unmount_fut = pin!(self.unmount_notify.notified().fuse());
        let mut read_fut = pin!(self.inner_read_vectored(header_buf, data_buf).fuse());

        select! {
            _ = unmount_fut => None,
            res = read_fut => Some(res)
        }
    }

    async fn inner_read_vectored<T: DerefMut<Target = [u8]> + Send + 'static>(
        &self,
        header_buf: Vec<u8>,
        data_buf: T,
    ) -> CompleteIoResult<(Vec<u8>, T), usize> {
        match &self.mode {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            ConnectionMode::Block(connection) => {
                connection
                    .read_vectored(&self.reads, header_buf, data_buf)
                    .await
            }
            #[cfg(any(
                all(target_os = "linux", feature = "unprivileged"),
                target_os = "freebsd"
            ))]
            ConnectionMode::NonBlock(connection) => {
                connection.read_vectored(header_buf, data_buf).await
            }
            #[cfg(test)]
            ConnectionMode::Memory(connection) => {
                connection.read_vectored(header_buf, data_buf).await
            }
        }
    }

    pub async fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        match &self.mode {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            ConnectionMode::Block(connection) => connection.write_vectored(bufs).await,
            #[cfg(any(
                all(target_os = "linux", feature = "unprivileged"),
                target_os = "freebsd"
            ))]
            ConnectionMode::NonBlock(connection) => connection.write_vectored(bufs).await,
            #[cfg(test)]
            ConnectionMode::Memory(connection) => connection.write_vectored(bufs),
        }
    }

    /// Writes one whole message from the calling thread, which the write may block.
    #[cfg(target_os = "linux")]
    pub(crate) fn write_vectored_blocking(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        match &self.mode {
            ConnectionMode::Block(connection) => (&*connection.file).write_vectored(bufs),
            #[cfg(feature = "unprivileged")]
            ConnectionMode::NonBlock(connection) => {
                uio::writev(&connection.fd, bufs).map_err(Into::into)
            }
            #[cfg(test)]
            ConnectionMode::Memory(connection) => connection.write_vectored(bufs),
        }
    }
}

#[derive(Debug)]
enum ConnectionMode {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Block(BlockFuseConnection),
    #[cfg(any(
        all(target_os = "linux", feature = "unprivileged"),
        target_os = "freebsd"
    ))]
    NonBlock(NonBlockFuseConnection),
    #[cfg(test)]
    Memory(super::memory::MemoryConnection),
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
struct BlockFuseConnection {
    /// Shared with each read worker, so the descriptor stays open until the worker exits.
    file: Arc<File>,
    read: Mutex<()>,
    // Linux v6.12 fuse_dev_write handles concurrent whole-message writes independently.
    #[cfg(not(target_os = "linux"))]
    write: Mutex<()>,
    /// Each read worker waits on this twice before its syscall.
    #[cfg(test)]
    gate: Option<Arc<std::sync::Barrier>>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl BlockFuseConnection {
    #[cfg(target_os = "linux")]
    pub fn new() -> io::Result<Self> {
        const DEV_FUSE: &str = "/dev/fuse";

        let file = OpenOptions::new().write(true).read(true).open(DEV_FUSE)?;

        Ok(Self::from_file(file))
    }

    fn from_file(file: File) -> Self {
        Self {
            file: Arc::new(file),
            read: Mutex::new(()),
            #[cfg(not(target_os = "linux"))]
            write: Mutex::new(()),
            #[cfg(test)]
            gate: None,
        }
    }

    #[cfg(target_os = "macos")]
    async fn new_with_unprivileged(
        mount_options: MountOptions,
        mount_path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        use std::{thread, time::Duration};

        use crate::find_macfuse_mount;

        let (sock0, sock1) = match socket::socketpair(
            AddressFamily::Unix,
            SockType::Stream,
            None,
            SockFlag::empty(),
        ) {
            Err(err) => return Err(err.into()),

            Ok((sock0, sock1)) => (sock0, sock1),
        };

        let binary_path = find_macfuse_mount()?;

        const ENV: &str = "_FUSE_COMMFD";

        let options = mount_options.build();

        debug!("mount options {:?}", options);

        let exec_path = match env::current_exe() {
            Ok(path) => path,
            Err(err) => return Err(err),
        };

        let mount_path = mount_path.as_ref().as_os_str().to_os_string();
        async_global_executor::spawn(async move {
            debug!("mount_thread start");
            let fd0 = sock0.as_raw_fd();
            let mut binding = Command::new(binary_path);
            let mut child = binding
                .env(ENV, fd0.to_string())
                .env("_FUSE_CALL_BY_LIB", "1")
                .env("_FUSE_COMMVERS", "2")
                .env("_FUSE_DAEMON_PATH", exec_path)
                .args(vec![options, mount_path])
                .spawn()?;
            if !child.status().await?.success() {
                return Err(io::Error::other("fusermount run failed"));
            }
            Ok(())
        });

        let fd1 = sock1.as_raw_fd();
        let fd = async_global_executor::spawn_blocking(move || {
            debug!("wait_thread start");
            // wait for macfuse mount command start
            // it seems that socket::recvmsg will not block to wait for the message
            // so we need to sleep for a while
            thread::sleep(Duration::from_secs(1));
            // let mut buf = vec![0; 10000]; // buf should large enough
            let mut buf = vec![]; // it seems 0 len still works well

            let mut cmsg_buf = nix::cmsg_space!([RawFd; 1]);

            let mut bufs = [IoSliceMut::new(&mut buf)];

            let msg = match socket::recvmsg::<()>(
                fd1,
                &mut bufs[..],
                Some(&mut cmsg_buf),
                MsgFlags::empty(),
            ) {
                Err(err) => return Err(err.into()),

                Ok(msg) => msg,
            };

            let mut cmsgs = match msg.cmsgs() {
                Err(err) => return Err(err.into()),
                Ok(cmsgs) => cmsgs,
            };

            let fd = if let Some(ControlMessageOwned::ScmRights(fds)) = cmsgs.next() {
                if fds.is_empty() {
                    return Err(io::Error::other("no fuse fd"));
                }

                fds[0]
            } else {
                return Err(io::Error::other("get fuse fd failed"));
            };

            Ok(fd)
        })
        .await
        .unwrap();

        // Safety: fd is valid
        let file = unsafe { File::from_raw_fd(fd) };

        Ok(Self::from_file(file))
    }

    async fn read_vectored<T: DerefMut<Target = [u8]> + Send + 'static>(
        &self,
        reads: &Arc<BlockingReads>,
        mut header_buf: Vec<u8>,
        mut data_buf: T,
    ) -> CompleteIoResult<(Vec<u8>, T), usize> {
        use std::io::Read;

        let _guard = self.read.lock().await;
        let Some(read) = reads.start() else {
            return ((header_buf, data_buf), Err(shut_down_error()));
        };
        let file = self.file.clone();
        #[cfg(test)]
        let gate = self.gate.clone();

        // A dropped Task drops a queued closure but does not stop a started one.
        let ((header_buf, data_buf), res) = async_global_executor::spawn_blocking(move || {
            #[cfg(test)]
            if let Some(gate) = gate {
                gate.wait();
                gate.wait();
            }

            let res = (&*file).read_vectored(&mut [
                IoSliceMut::new(&mut header_buf),
                IoSliceMut::new(&mut data_buf),
            ]);
            drop(file);
            drop(read);

            ((header_buf, data_buf), res)
        })
        .await;

        ((header_buf, data_buf), res)
    }

    async fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        #[cfg(not(target_os = "linux"))]
        let _guard = self.write.lock().await;
        (&*self.file).write_vectored(bufs)
    }
}

#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "freebsd"
))]
#[derive(Debug)]
struct NonBlockFuseConnection {
    fd: Async<OwnedFd>,
    read: Mutex<()>,
    #[cfg(target_os = "freebsd")]
    write: Mutex<()>,
}

#[cfg(any(
    all(target_os = "linux", feature = "unprivileged"),
    target_os = "freebsd"
))]
impl NonBlockFuseConnection {
    #[cfg(target_os = "freebsd")]
    fn new() -> io::Result<Self> {
        const DEV_FUSE: &str = "/dev/fuse";

        let file = OpenOptions::new().write(true).read(true).open(DEV_FUSE)?;

        Ok(Self {
            fd: Async::new(file.into())?,
            read: Mutex::new(()),
            write: Mutex::new(()),
        })
    }

    #[cfg(all(target_os = "linux", feature = "unprivileged"))]
    async fn new_with_unprivileged(
        mount_options: MountOptions,
        mount_path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        use std::os::fd::{AsRawFd, FromRawFd};

        let (sock0, sock1) = match socket::socketpair(
            AddressFamily::Unix,
            SockType::SeqPacket,
            None,
            SockFlag::empty(),
        ) {
            Err(err) => return Err(err.into()),

            Ok((sock0, sock1)) => (sock0, sock1),
        };

        let binary_path = find_fusermount3()?;

        const ENV: &str = "_FUSE_COMMFD";

        let options = mount_options.build_with_unprivileged();

        debug!("mount options {:?}", options);

        let mount_path = mount_path.as_ref().as_os_str().to_os_string();

        let fd0 = sock0.as_raw_fd();
        let mut child = Command::new(binary_path)
            .env(ENV, fd0.to_string())
            .args(vec![OsString::from("-o"), options, mount_path])
            .spawn()?;

        if !child.status().await?.success() {
            return Err(io::Error::other("fusermount run failed"));
        }

        let fd1 = sock1.as_raw_fd();
        let fd = async_global_executor::spawn_blocking(move || {
            // let mut buf = vec![0; 10000]; // buf should large enough
            let mut buf = vec![]; // it seems 0 len still works well

            let mut cmsg_buf = nix::cmsg_space!([RawFd; 1]);

            let mut bufs = [IoSliceMut::new(&mut buf)];

            let msg = match socket::recvmsg::<()>(
                fd1,
                &mut bufs[..],
                Some(&mut cmsg_buf),
                MsgFlags::empty(),
            ) {
                Err(err) => return Err(err.into()),

                Ok(msg) => msg,
            };

            let fd = if let Some(ControlMessageOwned::ScmRights(fds)) = msg.cmsgs()?.next() {
                if fds.is_empty() {
                    return Err(io::Error::other("no fuse fd"));
                }

                fds[0]
            } else {
                return Err(io::Error::other("get fuse fd failed"));
            };

            Ok(fd)
        })
        .await?;

        // Safety: fd is valid
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };

        Ok(Self {
            fd: Async::new(fd)?,
            read: Mutex::new(()),
        })
    }

    async fn read_vectored<T: DerefMut<Target = [u8]> + Send + 'static>(
        &self,
        mut header_buf: Vec<u8>,
        mut data_buf: T,
    ) -> CompleteIoResult<(Vec<u8>, T), usize> {
        let _guard = self.read.lock().await;

        let res = self
            .fd
            .read_with(|fd| {
                uio::readv(
                    fd,
                    &mut [
                        IoSliceMut::new(&mut header_buf),
                        IoSliceMut::new(&mut data_buf),
                    ],
                )
                .map_err(Into::into)
            })
            .await;

        ((header_buf, data_buf), res)
    }

    async fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        #[cfg(target_os = "freebsd")]
        let _guard = self.write.lock().await;
        uio::writev(&self.fd, bufs).map_err(Into::into)
    }
}

impl AsFd for FuseConnection {
    fn as_fd(&self) -> BorrowedFd<'_> {
        match &self.mode {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            ConnectionMode::Block(connection) => {
                // Safety: we own the File
                connection.file.as_fd()
            }

            #[cfg(any(
                all(target_os = "linux", feature = "unprivileged"),
                target_os = "freebsd"
            ))]
            ConnectionMode::NonBlock(connection) => connection.fd.as_fd(),

            #[cfg(test)]
            ConnectionMode::Memory(_) => unreachable!("only mount code asks for the device fd"),
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::pin::pin;
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    use futures_util::FutureExt;

    use super::BlockFuseConnection;
    use crate::raw::connection::BlockingReads;
    use crate::raw::connection::test_util::{pair, unread, within};

    const LIMIT: Duration = Duration::from_secs(5);

    /// Polls a read once, waits until its worker has entered, then drops the future, so only
    /// the worker is left. A dropped Task would cancel a worker that had not started.
    fn start_and_cancel(connection: &BlockFuseConnection, reads: &Arc<BlockingReads>) {
        let mut read = pin!(connection.read_vectored(reads, vec![0; 16], vec![0; 16]));
        assert!(read.as_mut().now_or_never().is_none());
        let gate = connection
            .gate
            .as_ref()
            .expect("async-io tests gate every read");
        gate.wait();
    }

    #[test]
    fn started_read_keeps_its_descriptor_after_cancel_and_drop() {
        async_global_executor::block_on(async {
            let (ours, mut peer) = pair();
            let gate = Arc::new(Barrier::new(2));
            let mut connection = BlockFuseConnection::from_file(ours);
            connection.gate = Some(gate.clone());
            let original = connection.file.as_raw_fd();
            let reads = Arc::new(BlockingReads::default());

            start_and_cancel(&connection, &reads);
            drop(connection);

            let (reused, mut reused_peer) = pair();
            assert_ne!(reused.as_raw_fd(), original);
            assert_ne!(reused_peer.as_raw_fd(), original);
            peer.write_all(b"original").expect("write original");
            reused_peer.write_all(b"reused").expect("write reused");
            gate.wait();

            assert!(within(LIMIT, reads.exited()).await.is_some());
            assert_eq!(unread(reused), b"reused");
        });
    }

    #[test]
    fn in_flight_read_ends_only_when_released_and_closed_reads_do_not_start() {
        async_global_executor::block_on(async {
            let (ours, peer) = pair();
            let gate = Arc::new(Barrier::new(2));
            let mut connection = BlockFuseConnection::from_file(ours);
            connection.gate = Some(gate.clone());
            let reads = Arc::new(BlockingReads::default());

            start_and_cancel(&connection, &reads);
            gate.wait();
            reads.close();
            let waited = within(Duration::from_millis(200), reads.exited()).await;
            assert!(waited.is_none(), "nothing released the read");

            let (_, res) = connection
                .read_vectored(&reads, vec![0; 16], vec![0; 16])
                .await;
            assert_eq!(res.expect_err("closed").raw_os_error(), Some(libc::ENODEV));

            // Closing the peer stands in for the unmount that releases the read.
            drop(peer);
            assert!(within(LIMIT, reads.exited()).await.is_some());
        });
    }
}
