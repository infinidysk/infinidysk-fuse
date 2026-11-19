//! Opt-in Linux checks that a root `Session::mount` tears down while its blocking read waits.
#![cfg(all(
    target_os = "linux",
    feature = "tokio-runtime",
    not(feature = "async-io-runtime")
))]
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use infinidysk_fuse::raw::MountHandle;
use infinidysk_fuse::{MountOptions, Result, raw::prelude::*};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Notify;
use tokio::time::{sleep, timeout};

const WAIT: Duration = Duration::from_secs(10);

/// Accepts INIT and nothing else. `destroy` needs the blocking pool, and waits for `hold` if set.
#[derive(Default)]
struct Idle {
    hold: Option<Arc<Notify>>,
}

impl Filesystem for Idle {
    async fn init(&self, _: Request) -> Result<ReplyInit> {
        Ok(ReplyInit::new(
            NonZeroU32::new(16 * 1024).expect("16 KiB is non-zero"),
        ))
    }

    async fn destroy(&self, _: Request) {
        if let Some(hold) = &self.hold {
            hold.notified().await;
        }
        tokio::task::spawn_blocking(|| ()).await.unwrap();
    }

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

/// One blocking thread, so anything that holds it during teardown starves `destroy`.
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

/// Mounts `fs` and returns once a lookup round trip shows dispatch runs.
async fn mount_idle(fs: Idle, path: &Path) -> MountHandle {
    let mut options = MountOptions::default();
    options.fs_name("infinidysk-fuse-direct-unmount-test");
    let handle = Session::new(options)
        .mount(fs, path)
        .await
        .expect("explicitly requested kernel test requires working FUSE");
    // A plain thread, so the lookup does not depend on the runtime under test.
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
            let handle = mount_idle(Idle::default(), directory.path()).await;
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
            drop(mount_idle(Idle::default(), directory.path()).await);
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

/// Waits on a plain thread until `path` is no longer mounted.
fn wait_unmounted(path: &Path) {
    while is_mounted(path) {
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires root and Linux /dev/fuse; run explicitly with --ignored"]
fn dropping_the_handle_and_then_the_runtime_still_unmounts() {
    if !can_mount() {
        eprintln!("skipped: needs root and /dev/fuse");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let runtime = one_blocking_thread();

    let handle = watchdog("mount", || {
        runtime.block_on(mount_idle(Idle::default(), directory.path()))
    });
    watchdog("drop the handle and the runtime", || {
        drop(handle);
        drop(runtime);
    });
    watchdog("unmount after the runtime dropped", || {
        wait_unmounted(directory.path())
    });
}

#[test]
#[ignore = "requires root and Linux /dev/fuse; run explicitly with --ignored"]
fn cancelled_unmount_still_unmounts() {
    if !can_mount() {
        eprintln!("skipped: needs root and /dev/fuse");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let runtime = one_blocking_thread();
    let hold = Arc::new(Notify::new());

    watchdog("cancel the unmount, then release destroy", || {
        runtime.block_on(async {
            let fs = Idle {
                hold: Some(hold.clone()),
            };
            let handle = mount_idle(fs, directory.path()).await;
            timeout(Duration::from_millis(200), handle.unmount())
                .await
                .expect_err("destroy is held, so the unmount cannot finish yet");
            hold.notify_one();
        })
    });
    watchdog("unmount after cancel", || wait_unmounted(directory.path()));
    watchdog("runtime drop", || drop(runtime));
}
