//! 宿主磁盘剩余空间(只读查询;v555 `PRODUCT[COM=34]` 的数据源)。
//!
//! 官方 `0x45c1d0` 是 Win32 `GetDiskFreeSpaceExA(path, &free, …)`,把
//! “调用方可用字节数”写 LET。内核不把 Win32 调用放进 `yu-rt`,由引擎装配层
//! 在启动时按存档根查询一次,经 `BootConfig::disk_free_bytes` 注入。
//!
//! 平台:
//! - Windows:`GetDiskFreeSpaceExW`;
//! - Unix/Android:`statvfs` 的 `f_bavail * f_frsize`;
//! - 其它(wasm):`None`(`PRODUCT[COM=34]` 写 0 并留 aligned-stub)。

use std::path::Path;

/// 查询 `path` 所在卷的“调用方可用字节数”。失败/不支持 = `None`。
#[must_use]
pub fn free_bytes(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut free: u64 = 0;
        // SAFETY: `wide` 以 NUL 结尾且在本调用期间存活;三个输出指针中只有
        // `free` 非空,其余允许为空。
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok != 0 {
            Some(free)
        } else {
            None
        }
    }
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `statvfs` 只写 `st`;`c` 是本调用期间存活的 NUL 结尾路径。
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
            Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
        } else {
            None
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = path;
        None
    }
}
