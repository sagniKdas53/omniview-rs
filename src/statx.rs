use std::ffi::CString;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct StatxTimestamp {
    tv_sec: i64,
    tv_nsec: u32,
    __reserved: i32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct StatxStruct {
    stx_mask: u32,
    stx_blksize: u32,
    stx_attributes: u64,
    stx_nlink: u32,
    stx_uid: u32,
    stx_gid: u32,
    stx_mode: u16,
    __spare0: [u16; 1],
    stx_ino: u64,
    stx_size: u64,
    stx_blocks: u64,
    stx_attributes_mask: u64,
    stx_atime: StatxTimestamp,
    stx_btime: StatxTimestamp,
    stx_ctime: StatxTimestamp,
    stx_mtime: StatxTimestamp,
    stx_rdev_major: u32,
    stx_rdev_minor: u32,
    stx_dev_major: u32,
    stx_dev_minor: u32,
    stx_mnt_id: u64,
    __spare2: u64,
    __spare3: [u64; 12],
}

const SYS_STATX: i64 = 332;
const AT_FDCWD: i32 = -100;
const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
const STATX_BTIME: u32 = 0x800;

pub fn get_file_ctime<P: AsRef<Path>>(path: P, meta: &Metadata) -> f64 {
    let p = path.as_ref();
    if let Ok(c_path) = CString::new(p.to_string_lossy().as_bytes()) {
        let mut sx: StatxStruct = unsafe { std::mem::zeroed() };
        let res = unsafe {
            libc::syscall(
                SYS_STATX,
                AT_FDCWD,
                c_path.as_ptr(),
                AT_SYMLINK_NOFOLLOW,
                STATX_BTIME,
                &mut sx as *mut StatxStruct,
            )
        };
        if res == 0 && (sx.stx_mask & STATX_BTIME) != 0 {
            let btime = sx.stx_btime.tv_sec as f64 + (sx.stx_btime.tv_nsec as f64 * 1e-9);
            if btime > 0.0 {
                return btime;
            }
        }
    }
    meta.ctime() as f64 + (meta.ctime_nsec() as f64 * 1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statx_size() {
        assert_eq!(std::mem::size_of::<StatxStruct>(), 256);
    }
}
