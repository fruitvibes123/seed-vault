#![cfg_attr(not(test), deny(clippy::unwrap_used))]
//! Born-at-mode private file/dir creation + fd-verified mode re-assertion.
//!
//! Three primitives for secrets-adjacent filesystem paths. Each closes the
//! same two gaps:
//!
//! - **No umask-default window.** Creation passes the mode into the create
//!   syscall itself (`DirBuilder::mode` / `OpenOptions::mode`), so the inode
//!   is born no wider than `mode`, then an `fchmod` on the fresh fd repairs
//!   any `& ~umask` narrowing back to *exactly* `mode`. There is no instant
//!   where the inode exists at a wider mode waiting for a follow-up chmod —
//!   the intermediate state is only ever narrower than `mode`, never wider.
//! - **No path-race on re-assertion.** Mode verification opens with
//!   `O_NOFOLLOW` (a planted symlink fails to open), stats the **fd**, and
//!   repairs drift with `fchmod` on that same fd — never a second path-based
//!   stat or chmod that could be redirected between calls. The whole low-12
//!   permission word is compared (`& 0o7777`), so a setuid/setgid/sticky bit
//!   that drifts onto a secrets inode is repaired away, not masked off and
//!   left in place.
//!
//! [`open_verified`] additionally returns the verified fd itself so callers
//! read through it — re-opening by path after the check would reintroduce
//! the TOCTOU window the fd discipline exists to close — and rejects any
//! non-regular-file inode (a directory / FIFO / socket / device swapped in at
//! the path, which `O_NOFOLLOW` alone does not catch). It opens `O_NONBLOCK`
//! so a hostile FIFO or device cannot stall the open before that check runs.
//!
//! `mode` is a raw `u32`: this crate does not constrain it to owner-only bits,
//! so passing a world-readable mode (`0o644`) to a secrets path is a caller
//! error the API does not catch. The full low-12 bits are compared and
//! repaired, so a setuid/setgid/sticky bit that has *drifted* onto an inode is
//! stripped back to `mode` (clearing bits is always permitted to the owner).
//! The API is built for owner-only modes, though: an `expected_mode` that
//! itself *sets* setgid is not reliably honoured — the kernel silently clears
//! `S_ISGID` on `fchmod` when the file's group is not the caller's — so such a
//! mode would never converge.
//!
//! On non-Unix targets the fns fall back to the plain std operations
//! (`create_dir_all` / `create_new` / `open`) and **mode enforcement is
//! silently absent** — mode bits are a Unix concept; callers needing private
//! modes elsewhere must enforce them by other means.
//!
//! All fns return `io::Result` — policy on failure (panic at startup,
//! propagate, retry) belongs to the caller.

use std::fs::File;
use std::io;
use std::path::Path;

/// `fchmod` `f` to exactly `mode`; map a failed syscall to `io::Error`.
///
/// The single `unsafe` site in the crate — every born-at-mode primitive
/// repairs through here so the fd-ownership invariant is reviewed in one place.
#[cfg(unix)]
fn fchmod_to(f: &File, mode: u32) -> io::Result<()> {
    use std::os::fd::AsRawFd;
                                                                                
                                                                          
    let rc = unsafe { libc::fchmod(f.as_raw_fd(), mode) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Create `path` as a directory born at `mode` (recursive, `DirBuilder::mode`),
/// then re-assert: open `O_NOFOLLOW | O_DIRECTORY`, stat the fd, `fchmod` if
/// drifted. Safe to call when the directory already exists — an existing
/// directory is repaired to `mode` rather than recreated.
///
/// With `recursive`, only the **terminal** component is re-asserted via
/// `fchmod`; intermediate components are born at `mode & ~umask` (never wider
/// than `mode`) but are not repaired to exactly `mode`.
///
/// The `O_NOFOLLOW` symlink guard assumes `path` has **no trailing slash**: a
/// trailing `/` makes the kernel resolve a final symlink as a directory
/// reference, so `O_NOFOLLOW` is ignored and a planted symlink-to-dir would be
/// followed. Callers must pass paths without a trailing separator.
#[cfg(unix)]
pub fn ensure_dir_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::fs::OpenOptions;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(mode)
        .create(path)?;
                                                                      
                                                                           
                                                                    
                                                                             
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)?;
                                                                            
                                                              
    if dir.metadata()?.mode() & 0o7777 != mode {
        fchmod_to(&dir, mode)?;
    }
    Ok(())
}

/// Non-Unix fallback: plain `create_dir_all`; `mode` is ignored.
#[cfg(not(unix))]
pub fn ensure_dir_mode(path: &Path, _mode: u32) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Exclusive create born at `mode`: `write(true).create_new(true)` +
/// `OpenOptions::mode(mode)` + `O_NOFOLLOW`, then `fchmod` the fresh fd to
/// exactly `mode`. The create syscall applies `mode & ~umask`; the `fchmod`
/// repairs any umask narrowing. There is no window where the file exists at
/// umask-default mode waiting for a follow-up chmod, and none where it is
/// wider than `mode` (the create-to-fchmod gap is only ever narrower).
///
/// Callers match `Err` with [`io::ErrorKind::AlreadyExists`] for the
/// lost-a-concurrent-bootstrap-race arm.
#[cfg(unix)]
pub fn create_exclusive(path: &Path, mode: u32) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    opts.mode(mode).custom_flags(libc::O_NOFOLLOW);
    let f = opts.open(path)?;
                                                                               
                                                                                
    fchmod_to(&f, mode)?;
    Ok(f)
}

/// Non-Unix fallback: plain exclusive create; `mode` is ignored.
#[cfg(not(unix))]
pub fn create_exclusive(path: &Path, _mode: u32) -> io::Result<File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Open `path` read-only with `O_NOFOLLOW | O_NONBLOCK`, confirm it is a
/// regular file (a directory / FIFO / socket / device swapped in at the path
/// is rejected — `O_NONBLOCK` keeps a hostile FIFO or device from stalling the
/// open before that check can run), verify its mode **on the fd** (`fchmod`
/// back to `expected_mode` if drifted), and return that SAME fd for the caller
/// to read. No path re-open: a by-path read after the check would let the file
/// be swapped for a symlink between the chmod and the read, redirecting the
/// read to attacker-controlled bytes.
///
/// The returned fd does not signal whether the mode was already correct or was
/// repaired; a caller that wants to alarm on drift must stat before calling.
#[cfg(unix)]
pub fn open_verified(path: &Path, expected_mode: u32) -> io::Result<File> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let f = std::fs::OpenOptions::new()
        .read(true)
                                                                             
                                                                                
                                                                              
                                                                                 
                                                                               
                       
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = f.metadata()?;
                                                                                
                                                                               
                                                                             
                                                                           
                                                         
    if !meta.file_type().is_file() {
        return Err(io::Error::other("open_verified: not a regular file"));
    }
                                                                            
                                                              
    if meta.mode() & 0o7777 != expected_mode {
        fchmod_to(&f, expected_mode)?;
    }
    Ok(f)
}

/// Non-Unix fallback: plain read-only open; `expected_mode` is ignored.
#[cfg(not(unix))]
pub fn open_verified(path: &Path, _expected_mode: u32) -> io::Result<File> {
    std::fs::OpenOptions::new().read(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;

                                                                              
                                                                       
                                                                           
                                                                           
                                                                    
                                                                         
    static UMASK_GUARD: RwLock<()> = RwLock::new(());

                                                                         
                                                                           
                                                                               
    fn fs_guard() -> std::sync::RwLockReadGuard<'static, ()> {
        UMASK_GUARD.read().unwrap_or_else(|e| e.into_inner())
    }
    fn umask_guard() -> std::sync::RwLockWriteGuard<'static, ()> {
        UMASK_GUARD.write().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    #[cfg(unix)]
    fn create_exclusive_file_born_at_0600() {
        use std::os::unix::fs::MetadataExt;
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.key");
        drop(create_exclusive(&path, 0o600).unwrap());
        let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600, "file must be born at 0600, no umask window");
    }

    #[test]
    #[cfg(unix)]
    fn create_exclusive_repairs_umask_narrowing_to_exact_mode() {
        use std::os::unix::fs::MetadataExt;
                                                                         
        let _g = umask_guard();
        let dir = tempfile::tempdir().unwrap();                               
        let path = dir.path().join("secret.key");
                                                                           
        let old = unsafe { libc::umask(0o277) };
        let created = create_exclusive(&path, 0o600);
        unsafe {
            libc::umask(old);
        }
        drop(created.unwrap());
        let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(
            mode, 0o600,
            "umask narrowing must be repaired back to exactly 0o600"
        );
    }

    #[test]
    #[cfg(unix)]
    fn create_exclusive_refuses_planted_symlink() {
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        let target = dir.path().join("would-be-target");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = create_exclusive(&link, 0o600).unwrap_err();
                                                                      
                                                     
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(
            !target.exists(),
            "must not follow the symlink to create its target"
        );
    }

    #[test]
    #[cfg(unix)]
    fn ensure_dir_mode_dir_born_at_0700() {
        use std::os::unix::fs::MetadataExt;
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets");
        ensure_dir_mode(&path, 0o700).unwrap();
        let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o700, "dir must be born at 0700");
    }

    #[test]
    #[cfg(unix)]
    fn ensure_dir_mode_repairs_drifted_dir() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets");
        ensure_dir_mode(&path, 0o700).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_dir_mode(&path, 0o700).unwrap();
        let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o700, "drifted dir mode must be repaired");
    }

    #[test]
    #[cfg(unix)]
    fn ensure_dir_mode_refuses_planted_symlink_at_target() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-dir");
        std::fs::create_dir(&real).unwrap();
                                                                               
                                                                
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
                                                                           
                                                                          
                                                                      
        assert!(
            ensure_dir_mode(&link, 0o700).is_err(),
            "O_NOFOLLOW must refuse a symlinked directory path"
        );
        let real_mode = std::fs::metadata(&real).unwrap().mode() & 0o777;
        assert_eq!(
            real_mode, 0o755,
            "must not have fchmod'd the target through the symlink"
        );
    }

    #[test]
    #[cfg(unix)]
    fn open_verified_repairs_chmod_644_back_to_0600_and_reads_same_fd() {
        use std::io::{Read, Write};
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.key");
        {
            let mut f = create_exclusive(&path, 0o600).unwrap();
            f.write_all(b"key-bytes").unwrap();
        }
                                                                    
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut f = open_verified(&path, 0o600).unwrap();
        let mode = std::fs::metadata(&path).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600, "644-drifted file must be repaired to 0600");
        let mut contents = String::new();
        f.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "key-bytes", "returned fd reads the file");
    }

    #[test]
    #[cfg(unix)]
    fn open_verified_strips_attacker_set_special_bits() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.key");
        drop(create_exclusive(&path, 0o600).unwrap());
                                                                             
                                                              
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o4600)).unwrap();
        drop(open_verified(&path, 0o600).unwrap());
        let mode = std::fs::metadata(&path).unwrap().mode() & 0o7777;
        assert_eq!(
            mode, 0o600,
            "setuid/setgid/sticky drift must be repaired to exactly 0o600"
        );
    }

    #[test]
    #[cfg(unix)]
    fn open_verified_refuses_directory_at_path() {
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.key");
                                                                         
                                                                                 
                         
        std::fs::create_dir(&path).unwrap();
        assert!(
            open_verified(&path, 0o600).is_err(),
            "a directory at a regular-file path must be rejected, not chmod'd and returned Ok"
        );
    }

    #[test]
    #[cfg(unix)]
    fn open_verified_rejects_fifo_without_blocking() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::sync::mpsc;
        use std::time::Duration;
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.key");
                                                                             
                                                                              
                                                                             
                                                                
        let cpath = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(
            unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) },
            0,
            "mkfifo must succeed: {}",
            io::Error::last_os_error()
        );
        let (tx, rx) = mpsc::channel();
        let probe = path.clone();
        std::thread::spawn(move || {
                                                                               
                                                                                 
            let _ = tx.send(open_verified(&probe, 0o600).is_err());
        });
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(rejected) => assert!(rejected, "FIFO must be rejected with an Err, not opened"),
            Err(_) => panic!("open_verified blocked on a planted FIFO (missing O_NONBLOCK)"),
        }
    }

    #[test]
    fn create_exclusive_on_existing_path_is_already_exists() {
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.key");
        drop(create_exclusive(&path, 0o600).unwrap());
        let err = create_exclusive(&path, 0o600).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    #[cfg(unix)]
    fn open_verified_refuses_planted_symlink() {
        let _g = fs_guard();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real-file");
        std::fs::write(&target, "x").unwrap();
        let link = dir.path().join("planted-link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(
            open_verified(&link, 0o600).is_err(),
            "O_NOFOLLOW must refuse a symlink (ELOOP)"
        );
    }
}
