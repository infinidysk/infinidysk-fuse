//! Opt-in Linux check that children do not inherit the FUSE device a mount helper hands back.
#![cfg(all(
    target_os = "linux",
    feature = "tokio-runtime",
    not(feature = "async-io-runtime"),
    feature = "unprivileged"
))]
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use infinidysk_fuse::{MountOptions, Result, raw::prelude::*};
use tokio::sync::Notify;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);

/// Serves nothing; signals once INIT reaches it.
struct Empty(Arc<Notify>);

impl Filesystem for Empty {
    async fn init(&self, _: Request) -> Result<ReplyInit> {
        self.0.notify_one();
        Ok(ReplyInit::new(NonZeroU32::new(65536).unwrap()))
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

/// The descriptor targets the process `pid` holds open. Fails if any descriptor cannot be read.
fn open_targets(pid: u32) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::read_dir(format!("/proc/{pid}/fd"))?
        .map(|entry| std::fs::read_link(entry?.path()))
        .collect()
}

#[tokio::test]
#[ignore = "requires Linux /dev/fuse and fusermount3; run explicitly with --ignored"]
async fn children_do_not_inherit_the_fuse_device() {
    let directory = tempfile::tempdir().unwrap();
    let mut options = MountOptions::default();
    options.fs_name("infinidysk-fuse-fd-inherit-test");
    let init_called = Arc::new(Notify::new());
    let handle = Session::new(options)
        .mount_with_unprivileged(Empty(init_called.clone()), directory.path())
        .await
        .expect("explicitly requested kernel test requires working FUSE");
    timeout(WAIT, init_called.notified())
        .await
        .expect("INIT within the timeout");

    // `cat` waits on stdin, so the child stays alive while its descriptors are read.
    let mut child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn cat");
    // The echoed byte shows `cat` is in its read loop, so the loader no longer opens or closes files.
    let mut echo = [0; 1];
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(b"x")
        .expect("write to cat");
    child
        .stdout
        .as_mut()
        .expect("piped stdout")
        .read_exact(&mut echo)
        .expect("read the echo from cat");
    let targets = open_targets(child.id());
    drop(child.stdin.take());
    child.wait().expect("wait cat");

    timeout(WAIT, handle.unmount())
        .await
        .expect("unmount within the timeout")
        .expect("unmount");

    let targets = targets.expect("read every descriptor of the child");
    assert!(
        !targets
            .iter()
            .any(|target| target == Path::new("/dev/fuse")),
        "the child inherited the fuse device: {targets:?}"
    );
}
