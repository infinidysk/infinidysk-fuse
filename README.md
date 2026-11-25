# infinidysk-fuse

[![Cargo](https://img.shields.io/crates/v/infinidysk-fuse.svg)](https://crates.io/crates/infinidysk-fuse)
[![Documentation](https://docs.rs/infinidysk-fuse/badge.svg)](https://docs.rs/infinidysk-fuse)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

An async FUSE library for Rust. It talks to `/dev/fuse` directly, so it does
not link against `libfuse`.

It is a fork of [`fuse3`](https://github.com/Sherlock-Holo/fuse3), maintained
for the FUSE mount in [infinidysk](https://github.com/infinidysk/infinidysk),
a Usenet-backed virtual filesystem that streams media to players. The
streaming read path gets most of the attention here.

## Install

Enable exactly one runtime feature:

```toml
[dependencies]
infinidysk-fuse = { version = "0.10", features = ["tokio-runtime", "unprivileged"] }
```

The crate is imported as `infinidysk_fuse`.

| Feature | What it does |
|---------|--------------|
| `tokio-runtime` | Drive IO and tasks with [tokio](https://docs.rs/tokio). |
| `async-io-runtime` | Drive IO and tasks with [async-io](https://docs.rs/async-io) and [async-global-executor](https://docs.rs/async-global-executor). |
| `unprivileged` | Mount without root on Linux through the `fusermount3` binary. |
| `file-lock` | POSIX file locks (`getlk`, `setlk`). |

The two runtime features are mutually exclusive. The crate does not build
with neither.

## Kernel capabilities are opt-in (breaking change for 0.12)

The session no longer negotiates these INIT capabilities just because the
kernel offers them. Each one changes what the kernel expects from your
filesystem, so enable it only when your filesystem handles it:

| Option | Capability | Your filesystem must |
|--------|------------|----------------------|
| `MountOptions::atomic_o_trunc` | `FUSE_ATOMIC_O_TRUNC` | Truncate in `open` when the flags contain `O_TRUNC`. |
| `MountOptions::export_support` | `FUSE_EXPORT_SUPPORT` | Answer `lookup` of `.` and `..` and of inodes it has not handed out in this mount. Raw layer only; the path layer rejects it with `InvalidInput`. |
| `MountOptions::posix_acl` | `FUSE_POSIX_ACL` | Store and return the `system.posix_acl_*` extended attributes. |
| `MountOptions::cache_symlinks` | `FUSE_CACHE_SYMLINKS` | Keep symlink targets stable, or invalidate them through `notify`. |
| `MountOptions::case_insensitive` (macOS) | `FUSE_CASE_INSENSITIVE` | Match names in `lookup` without regard to case. |

All default to off. `default_permissions(true)` no longer turns on
`FUSE_POSIX_ACL`; call `posix_acl(true)` as well to keep ACL support.

To keep the 0.11 behavior, opt in to what your filesystem supports:

```rust
let mut options = MountOptions::default();
options
    .atomic_o_trunc(true)
    .posix_acl(true)
    .cache_symlinks(true);
```

## Notifications (breaking change for 0.11)

Every method on the notifier passed to `Filesystem::ready` now returns
`std::io::Result<()>`: `wakeup`, `invalid_inode`, `invalid_entry`, `delete`,
`store` and `retrieve`. Handle the result with `?` or explicit error handling.
The temporary `try_invalid_inode` method is replaced by `invalid_inode`.

Each call waits for its kernel write. Local kernel rejections return an error
without ending the session. Transport failures and incomplete writes return
an error and signal session teardown. Notification failures never send an
ordinary error reply with request ID zero.

For changed file bytes, await `notify.invalid_inode(inode, 0, 0)` and check
the result before finalizing publication. Coordinate in-flight reads too:
invalidation alone does not stop an old reply from filling the cache again.
For `retrieve`, success means the request was sent; data arrives later through
`Filesystem::notify_reply`. A failed `store` can leave partially stored data.

## Quick start

Implement `raw::Filesystem` (inode-based) or `path::PathFilesystem`
(path-based). Every method except `init` and `destroy` has a default that
replies `ENOSYS`.

```rust
use std::num::NonZeroU32;

use infinidysk_fuse::raw::prelude::*;
use infinidysk_fuse::{MountOptions, Result};

struct MyFs;

impl Filesystem for MyFs {
    async fn init(&self, _req: Request) -> Result<ReplyInit> {
        Ok(ReplyInit::new(NonZeroU32::new(1024 * 1024).expect("1 MiB is non-zero")))
    }

    async fn destroy(&self, _req: Request) {}

    // lookup, getattr, readdir, open, read, ...
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut options = MountOptions::default();
    options.fs_name("myfs").read_only(true);

    let handle = Session::new(options)
        .mount_with_unprivileged(MyFs, "/mnt/myfs")
        .await?;

    // Resolves when the filesystem is unmounted. Call `handle.unmount()`
    // to unmount from code instead.
    handle.await
}
```

Runnable examples are in [`examples/`](examples/src):

| Example | Layer | Shows |
|---------|-------|-------|
| [`helloworld`](examples/src/helloworld/main.rs) | raw | A read-only filesystem with one file. |
| [`memfs`](examples/src/memfs/main.rs) | raw | A writable in-memory filesystem. |
| [`path_memfs`](examples/src/path_memfs/main.rs) | path | The same in-memory filesystem on the path layer. |
| [`poll`](examples/src/poll/main.rs) | raw | `poll` and kernel notifications. |

```sh
mkdir -p /tmp/mnt
cargo run -p infinidysk-fuse-examples --bin helloworld -- /tmp/mnt
```

## Layers

- **`raw`** is close to the kernel protocol. Requests carry inode numbers, and
  you own the inode table and lookup counts. Use it when performance or exact
  kernel behavior matters.
- **`path`** sits on top of `raw`. Requests carry paths, and an inode bridge
  maps paths to inode numbers for you. It is simpler, with the limits listed
  below.

## Platforms

| OS | Kernel side | Mount |
|----|-------------|-------|
| Linux | in-tree `fuse` module | `mount` needs root (or `CAP_SYS_ADMIN`). `mount_with_unprivileged` uses `fusermount3` (the `fuse3` package). |
| macOS | [macFUSE](https://macfuse.github.io/) | Always through `mount_macfuse`; root is not needed. |
| FreeBSD | `fusefs` | `nmount`. Works without root when `vfs.usermount=1` and you can write to the mount point. |

CI builds all three. The mount point must be empty unless you set
`MountOptions::nonempty`.

## Supported

- Unprivileged mounts through `fusermount3`.
- `readdirplus`, to cut `lookup` round trips when listing directories.
- POSIX file locks (`file-lock` feature).
- Async direct IO, and the `no_open` / `no_open_dir` options.
- Opt-in kernel capabilities: atomic `O_TRUNC`, export support (raw layer),
  POSIX ACLs, symlink caching and, on macOS, case-insensitive names.
- Kernel notifications through `notify::Notify`: invalidate inode or entry,
  delete, store, retrieve and poll wakeup.

## Not supported

- `ioctl`.
- `fuseblk` (block-device-backed) mounts.

`poll` and `notify_reply` work but their API may still change.

## Known limits of the path layer

The path layer (`PathFilesystem` on top of the inode bridge) counts kernel
lookups per inode and keeps an inode number until the kernel forgets it. These
cases are still open:

- `link` gives the new name its own inode number instead of the number of the
  linked inode, so the kernel sees two inodes for one file.
- `PathFilesystem::batch_forget` only gets paths, not the `nlookup` of each
  inode, so a path file system cannot follow partial forgets.
- If a reply cannot be delivered to the kernel, the lookup counts taken for it
  are not given back, so those inode numbers stay taken until unmount.

## Minimum Rust version

Rust 1.91 (edition 2024). The `rust-version` in `Cargo.toml` is the source of
truth.

## License

MIT. See [LICENSE](LICENSE). Original work by Sherlock Holo in
[`fuse3`](https://github.com/Sherlock-Holo/fuse3).
