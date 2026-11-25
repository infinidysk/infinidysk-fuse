use std::io;
use std::path::Path;

use crate::MountOptions;
use crate::path::inode_path_bridge::InodePathBridge;
use crate::path::path_filesystem::PathFilesystem;
use crate::raw;

#[cfg(any(feature = "async-io-runtime", feature = "tokio-runtime"))]
#[derive(Debug)]
/// fuse filesystem session, path based.
pub struct Session {
    mount_options: MountOptions,
}

#[cfg(any(feature = "async-io-runtime", feature = "tokio-runtime"))]
impl Session {
    /// new a fuse filesystem session.
    pub fn new(mount_options: MountOptions) -> Self {
        Self { mount_options }
    }

    fn check_options(&self) -> io::Result<()> {
        if self.mount_options.export_support {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "export_support is not supported by the path layer",
            ));
        }

        Ok(())
    }

    #[cfg(feature = "unprivileged")]
    /// mount the filesystem without root permission.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidInput`](io::ErrorKind::InvalidInput) when
    /// [`MountOptions::export_support`] is enabled, which the path layer does not support.
    pub async fn mount_with_unprivileged<P, FS>(
        self,
        fs: FS,
        mount_path: P,
    ) -> io::Result<raw::MountHandle>
    where
        P: AsRef<Path>,
        FS: PathFilesystem + Send + Sync + 'static,
    {
        self.check_options()?;
        let bridge = InodePathBridge::new(fs);

        raw::Session::new(self.mount_options)
            .mount_with_unprivileged(bridge, mount_path)
            .await
    }

    /// mount the filesystem with root permission.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidInput`](io::ErrorKind::InvalidInput) when
    /// [`MountOptions::export_support`] is enabled, which the path layer does not support.
    pub async fn mount<P, FS>(self, fs: FS, mount_path: P) -> io::Result<raw::MountHandle>
    where
        P: AsRef<Path>,
        FS: PathFilesystem + Send + Sync + 'static,
    {
        self.check_options()?;
        let bridge = InodePathBridge::new(fs);

        raw::Session::new(self.mount_options)
            .mount(bridge, mount_path)
            .await
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "file-lock")]
    use std::ffi::OsStr;

    use super::*;
    use crate::Result;
    use crate::path::reply::ReplyInit;
    #[cfg(feature = "file-lock")]
    use crate::path::reply::ReplyLock;
    use crate::raw::Request;

    struct Unused;

    impl PathFilesystem for Unused {
        async fn init(&self, _req: Request) -> Result<ReplyInit> {
            unreachable!("the session must reject the options before mounting")
        }

        async fn destroy(&self, _req: Request) {}

        #[cfg(feature = "file-lock")]
        async fn getlk(
            &self,
            _req: Request,
            _path: Option<&OsStr>,
            _fh: u64,
            _lock_owner: u64,
            _start: u64,
            _end: u64,
            _type: u32,
            _pid: u32,
        ) -> Result<ReplyLock> {
            unreachable!()
        }

        #[cfg(feature = "file-lock")]
        async fn setlk(
            &self,
            _req: Request,
            _path: Option<&OsStr>,
            _fh: u64,
            _lock_owner: u64,
            _start: u64,
            _end: u64,
            _type: u32,
            _pid: u32,
            _block: bool,
        ) -> Result<()> {
            unreachable!()
        }
    }

    fn export_options() -> MountOptions {
        let mut options = MountOptions::default();
        options.export_support(true);
        options
    }

    // The mount point does not exist, so any error other than InvalidInput means the
    // session tried to mount.
    const MOUNT_PATH: &str = "/nonexistent/infinidysk-fuse-export-test";

    #[tokio::test]
    async fn mount_rejects_export_support() {
        let err = Session::new(export_options())
            .mount(Unused, MOUNT_PATH)
            .await
            .expect_err("export_support must be rejected");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(feature = "unprivileged")]
    #[tokio::test]
    async fn mount_with_unprivileged_rejects_export_support() {
        let err = Session::new(export_options())
            .mount_with_unprivileged(Unused, MOUNT_PATH)
            .await
            .expect_err("export_support must be rejected");

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
