//! Running processes and the files they hold open, read through macOS libproc.

use chrono::{DateTime, Utc};
use std::ffi::{CStr, c_int, c_void};
use std::mem::size_of;
use std::path::PathBuf;

const PROC_PIDFDVNODEPATHINFO: c_int = 2;

/// `struct proc_fileinfo` from `<sys/proc_info.h>`; libc does not define it.
#[repr(C)]
struct FileInfo {
    open_flags: u32,
    status: u32,
    offset: i64,
    kind: i32,
    guard_flags: u32,
}

/// `struct vnode_fdinfowithpath` from `<sys/proc_info.h>`.
#[repr(C)]
struct VnodeFdInfoWithPath {
    file: FileInfo,
    vnode: libc::vnode_info_path,
}

/// Every process id on the system.
pub fn all_pids() -> Vec<u32> {
    let mut capacity = 1024;
    loop {
        let mut pids = vec![0 as libc::pid_t; capacity];
        let bytes = (capacity * size_of::<libc::pid_t>()) as c_int;
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast::<c_void>(), bytes) };
        if count < 0 {
            return Vec::new();
        }
        let count = count as usize;
        // A full buffer may have cut the list short.
        if count < capacity {
            pids.truncate(count);
            return pids
                .into_iter()
                .filter(|&pid| pid > 0)
                .map(|pid| pid as u32)
                .collect();
        }
        capacity *= 2;
    }
}

/// The process's start time at whole-second resolution, or `None` if unavailable.
pub fn start_time(pid: u32) -> Option<DateTime<Utc>> {
    let pid = c_int::try_from(pid).ok().filter(|&pid| pid > 0)?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = size_of::<libc::proc_bsdinfo>() as c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if written != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    // Session identities store lstart timestamps with whole-second precision.
    DateTime::from_timestamp(i64::try_from(info.pbi_start_tvsec).ok()?, 0)
}

/// The process's command name, or `None` once it has exited.
pub fn name(pid: u32) -> Option<String> {
    let mut buffer = [0_u8; 2 * libc::MAXCOMLEN + 1];
    let length = unsafe {
        libc::proc_name(
            pid as c_int,
            buffer.as_mut_ptr().cast::<c_void>(),
            buffer.len() as u32,
        )
    };
    (length > 0).then(|| String::from_utf8_lossy(&buffer[..length as usize]).into_owned())
}

/// The path of the file open on `fd`, when it is a file on disk.
pub fn fd_path(pid: u32, fd: i32) -> Option<PathBuf> {
    let mut info = std::mem::MaybeUninit::<VnodeFdInfoWithPath>::zeroed();
    let size = size_of::<VnodeFdInfoWithPath>() as c_int;
    let written = unsafe {
        libc::proc_pidfdinfo(
            pid as c_int,
            fd,
            PROC_PIDFDVNODEPATHINFO,
            info.as_mut_ptr().cast::<c_void>(),
            size,
        )
    };
    if written != size {
        return None;
    }
    let info = unsafe { info.assume_init() };
    let path = unsafe { CStr::from_ptr(info.vnode.vip_path.as_ptr().cast()) };
    let path = path.to_str().ok()?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// The paths of every file on disk the process holds open.
pub fn open_files(pid: u32) -> Vec<PathBuf> {
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDLISTFDS,
            0,
            std::ptr::null_mut(),
            0,
        )
    };
    if bytes <= 0 {
        return Vec::new();
    }
    // Room for descriptors opened between the two calls.
    let capacity = bytes as usize / size_of::<libc::proc_fdinfo>() + 16;
    let mut fds = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0
        };
        capacity
    ];
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast::<c_void>(),
            (capacity * size_of::<libc::proc_fdinfo>()) as c_int,
        )
    };
    if bytes <= 0 {
        return Vec::new();
    }
    fds.truncate(bytes as usize / size_of::<libc::proc_fdinfo>());
    fds.into_iter()
        .filter(|fd| fd.proc_fdtype == libc::PROX_FDTYPE_VNODE as u32)
        .filter_map(|fd| fd_path(pid, fd.proc_fd))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn reads_process_start_time_at_second_resolution() {
        let before = chrono::Utc::now();
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let start = start_time(child.id());
        child.kill().unwrap();
        child.wait().unwrap();

        let start = start.expect("libproc must read the child process start time");
        assert!((start - before).num_milliseconds().abs() <= 2000);
        assert_eq!(start.timestamp_subsec_nanos(), 0);
        assert!(start_time(child.id()).is_none());
        assert!(start_time(std::process::id()).is_some());
        assert!(start_time(0).is_none());
        assert!(start_time(u32::MAX).is_none());
    }

    #[test]
    fn struct_matches_the_sdk_layout() {
        // sizeof(struct vnode_fdinfowithpath) in the macOS SDK.
        assert_eq!(size_of::<VnodeFdInfoWithPath>(), 1200);
    }

    #[test]
    fn reads_this_process() {
        let pid = std::process::id();
        assert!(all_pids().contains(&pid));
        assert!(name(pid).is_some_and(|name| !name.is_empty()));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("held");
        let _file = File::create(&path).unwrap();
        assert!(open_files(pid).contains(&path));
    }
}
