//! Opt-in Linux checks that a root `Session::mount` tears down while its blocking read waits.
#![cfg(all(
    target_os = "linux",
    feature = "tokio-runtime",
    not(feature = "async-io-runtime")
))]
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use infinidysk_fuse::raw::MountHandle;
use infinidysk_fuse::{MountOptions, Result, raw::prelude::*};
use tokio::runtime::{Builder, Runtime};
use tokio::time::{sleep, timeout};

const WAIT: Duration = Duration::from_secs(10);

/// Accepts INIT and nothing else.
struct Idle;

impl Filesystem for Idle {
    async fn init(&self, _: Request) -> Result<ReplyInit> {
        Ok(ReplyInit::new(
            NonZeroU32::new(16 * 1024).expect("16 KiB is non-zero"),
        ))
    }

    async fn destroy(&self, _: Request) {}

    #[cfg(feature = "file-lock")]
    async fn getlk(
        &self,
        _: Request,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
        _: u32,
        _: u32,
    ) -> Result<ReplyLock> {
        Err(libc::ENOSYS.into())
    }

    #[cfg(feature = "file-lock")]
    async fn setlk(
        &self,
        _: Request,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
        _: u32,
        _: u32,
        _: bool,
    ) -> Result<()> {
        Err(libc::ENOSYS.into())
    }
}

/// A direct mount needs root and the FUSE device.
fn can_mount() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    let root = unsafe { libc::geteuid() } == 0;
    root && Path::new("/dev/fuse").exists()
}

fn is_mounted(path: &Path) -> bool {
    let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
    let path = path.to_str().unwrap();
    mountinfo
        .lines()
        .any(|line| line.split(' ').nth(4) == Some(path))
}

/// One blocking thread, so a queued unmount would wait behind the blocked read forever.
fn one_blocking_thread() -> Runtime {
    Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

/// Runs `f`, and aborts the process if it does not return within [`WAIT`]. The watchdog is a
/// thread outside the runtime under test, so a stalled runtime cannot stall it.
fn watchdog<T>(what: &'static str, f: impl FnOnce() -> T) -> T {
    let (done, finished) = mpsc::channel::<()>();
    let dog = thread::spawn(move || {
        if finished.recv_timeout(WAIT) == Err(mpsc::RecvTimeoutError::Timeout) {
            // Not eprintln: the test harness captures it, and the abort would discard it.
            let message = format!("{what} did not finish within {WAIT:?}\n");
            let _ = std::io::Write::write_all(&mut std::io::stderr(), message.as_bytes());
            std::process::abort();
        }
    });
    let output = f();
    drop(done);
    dog.join().unwrap();
    output
}

/// Mounts [`Idle`] and returns once a lookup round trip shows dispatch runs.
async fn mount_idle(path: &Path) -> MountHandle {
    let mut options = MountOptions::default();
    options.fs_name("infinidysk-fuse-direct-unmount-test");
    let handle = Session::new(options)
        .mount(Idle, path)
        .await
        .expect("explicitly requested kernel test requires working FUSE");
    // A plain thread: the runtime's only blocking thread now waits in the device read.
    let missing = path.join("missing");
    thread::spawn(move || std::fs::metadata(missing))
        .join()
        .unwrap()
        .expect_err("Idle has no lookup");
    assert!(is_mounted(path));
    handle
}

#[test]
#[ignore = "requires root and Linux /dev/fuse; run explicitly with --ignored"]
fn unmount_returns_and_the_runtime_drops_with_one_blocking_thread() {
    if !can_mount() {
        eprintln!("skipped: needs root and /dev/fuse");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let runtime = one_blocking_thread();

    watchdog("mount and unmount", || {
        runtime.block_on(async {
            let handle = mount_idle(directory.path()).await;
            handle.unmount().await.expect("unmount");
        })
    });
    assert!(!is_mounted(directory.path()));
    watchdog("runtime drop", || drop(runtime));
}

#[test]
#[ignore = "requires root and Linux /dev/fuse; run explicitly with --ignored"]
fn dropping_the_handle_unmounts_and_the_runtime_drops_with_one_blocking_thread() {
    if !can_mount() {
        eprintln!("skipped: needs root and /dev/fuse");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let runtime = one_blocking_thread();

    watchdog("mount and drop the handle", || {
        runtime.block_on(async {
            drop(mount_idle(directory.path()).await);
            timeout(WAIT, async {
                while is_mounted(directory.path()) {
                    sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .expect("dropping the handle unmounts within the timeout");
        })
    });
    watchdog("runtime drop", || drop(runtime));
}
