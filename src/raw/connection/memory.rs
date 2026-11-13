//! In-process stand-in for `/dev/fuse`, so tests can drive the dispatcher without a mount.

use std::collections::VecDeque;
use std::io;
use std::io::IoSlice;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex as SyncMutex};

use futures_util::lock::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::CompleteIoResult;
use crate::raw::abi::FUSE_IN_HEADER_SIZE;

#[derive(Debug)]
pub(crate) struct MemoryConnection {
    requests: Mutex<UnboundedReceiver<io::Result<Vec<u8>>>>,
    replies: UnboundedSender<Vec<u8>>,
    write_errors: Arc<SyncMutex<VecDeque<i32>>>,
}

/// The kernel's end: push request frames (or scripted read errors) and read reply frames.
#[derive(Debug)]
pub(crate) struct KernelSide {
    pub(crate) requests: UnboundedSender<io::Result<Vec<u8>>>,
    pub(crate) replies: UnboundedReceiver<Vec<u8>>,
    write_errors: Arc<SyncMutex<VecDeque<i32>>>,
}

impl KernelSide {
    /// The next write fails with `errno` and is not recorded as a reply.
    pub(crate) fn fail_next_write(&self, errno: i32) {
        self.write_errors
            .lock()
            .expect("no test panics while holding the write-error lock")
            .push_back(errno);
    }
}

impl MemoryConnection {
    pub(crate) fn new() -> (Self, KernelSide) {
        let (request_tx, request_rx) = unbounded_channel();
        let (reply_tx, reply_rx) = unbounded_channel();
        let write_errors = Arc::new(SyncMutex::new(VecDeque::new()));

        (
            Self {
                requests: Mutex::new(request_rx),
                replies: reply_tx,
                write_errors: write_errors.clone(),
            },
            KernelSide {
                requests: request_tx,
                replies: reply_rx,
                write_errors,
            },
        )
    }

    /// Fills the buffers like `readv`. A closed request queue reads as `ENODEV`, like an unmounted device.
    pub(crate) async fn read_vectored<T: DerefMut<Target = [u8]>>(
        &self,
        mut header_buf: Vec<u8>,
        mut data_buf: T,
    ) -> CompleteIoResult<(Vec<u8>, T), usize> {
        let frame = match self.requests.lock().await.recv().await {
            None => Err(io::Error::from_raw_os_error(libc::ENODEV)),
            Some(frame) => frame,
        };

        let res = frame.map(|frame| {
            let header_len = frame.len().min(FUSE_IN_HEADER_SIZE).min(header_buf.len());
            header_buf[..header_len].copy_from_slice(&frame[..header_len]);

            let data = &frame[header_len..];
            let data_len = data.len().min(data_buf.len());
            data_buf[..data_len].copy_from_slice(&data[..data_len]);

            header_len + data_len
        });

        ((header_buf, data_buf), res)
    }

    /// Records one write as one frame, like a single `writev` to the device.
    pub(crate) fn write_vectored(&self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        let scripted = self
            .write_errors
            .lock()
            .expect("no test panics while holding the write-error lock")
            .pop_front();
        if let Some(errno) = scripted {
            return Err(io::Error::from_raw_os_error(errno));
        }

        let frame = bufs
            .iter()
            .flat_map(|buf| buf.iter().copied())
            .collect::<Vec<_>>();
        let len = frame.len();

        self.replies
            .send(frame)
            .map_err(|_| io::Error::from_raw_os_error(libc::ENODEV))?;

        Ok(len)
    }
}
