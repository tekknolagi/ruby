/// Jitdump file writer for integration with perf/samply.
///
/// Writes a jitdump binary file that profilers can read to map JIT code
/// addresses to function names and source lines (HIR instructions).
///
/// Format reference: https://github.com/torvalds/linux/blob/master/tools/perf/util/jitdump.h

use std::fs;
use std::io::{self, Write, BufWriter};
use std::sync::Mutex;
use std::time::Instant;

// Raw libc bindings for mmap (no libc crate dependency)
mod ffi {
    use std::os::raw::{c_void, c_int, c_long};
    pub const PROT_READ: c_int = 1;
    pub const PROT_EXEC: c_int = 4;
    pub const MAP_PRIVATE: c_int = 0x0002;
    pub const MAP_FAILED: *mut c_void = !0 as *mut c_void;
    #[cfg(target_os = "linux")]
    pub const _SC_PAGESIZE: c_int = 30;
    #[cfg(target_os = "macos")]
    pub const _SC_PAGESIZE: c_int = 29;
    unsafe extern "C" {
        pub fn mmap(addr: *mut c_void, len: usize, prot: c_int, flags: c_int, fd: c_int, offset: i64) -> *mut c_void;
        pub fn munmap(addr: *mut c_void, len: usize) -> c_int;
        pub fn sysconf(name: c_int) -> c_long;
    }
}

// Jitdump magic: "JiTD" in little-endian
const JITDUMP_MAGIC: u32 = 0x4A695444;
const JITDUMP_VERSION: u32 = 1;
const JITDUMP_HEADER_SIZE: u32 = 40;

// Record types
const JIT_CODE_LOAD: u32 = 0;
const JIT_CODE_DEBUG_INFO: u32 = 2;
const JIT_CODE_CLOSE: u32 = 3;

// ELF machine types
#[cfg(target_arch = "aarch64")]
const ELF_MACH: u32 = 183; // EM_AARCH64
#[cfg(target_arch = "x86_64")]
const ELF_MACH: u32 = 62; // EM_X86_64

struct JitdumpInner {
    file: BufWriter<fs::File>,
    epoch: Instant,
    code_index: u64,
    mmap_ptr: *mut std::os::raw::c_void,
    mmap_len: usize,
}

unsafe impl Send for JitdumpInner {}

pub struct JitdumpWriter {
    inner: Mutex<JitdumpInner>,
}

/// A debug entry mapping a code offset to a source line.
pub struct DebugEntry<'a> {
    /// Offset from the start of the JIT code region
    pub code_addr: u64,
    /// 1-based line number in the source file
    pub line: u32,
    /// Source file path
    pub filename: &'a str,
}

impl JitdumpWriter {
    pub fn open() -> io::Result<Self> {
        let pid = std::process::id();
        let path = format!("/tmp/jit-{pid}.dump");

        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .read(true)
            .open(&path)?;

        let epoch = Instant::now();

        // Write the file header
        let mut writer = BufWriter::new(file);
        writer.write_all(&JITDUMP_MAGIC.to_le_bytes())?;
        writer.write_all(&JITDUMP_VERSION.to_le_bytes())?;
        writer.write_all(&JITDUMP_HEADER_SIZE.to_le_bytes())?;
        writer.write_all(&ELF_MACH.to_le_bytes())?;
        writer.write_all(&0u32.to_le_bytes())?; // pad1
        writer.write_all(&pid.to_le_bytes())?;
        writer.write_all(&0u64.to_le_bytes())?; // timestamp
        writer.write_all(&0u64.to_le_bytes())?; // flags
        writer.flush()?;

        // mmap the file so profilers can discover it via /proc/pid/maps
        use std::os::unix::io::AsRawFd;
        let fd = writer.get_ref().as_raw_fd();
        let page_size = unsafe { ffi::sysconf(ffi::_SC_PAGESIZE) as usize };
        let mmap_ptr = unsafe {
            ffi::mmap(
                std::ptr::null_mut(),
                page_size,
                ffi::PROT_READ | ffi::PROT_EXEC,
                ffi::MAP_PRIVATE,
                fd,
                0,
            )
        };
        if mmap_ptr == ffi::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            inner: Mutex::new(JitdumpInner {
                file: writer,
                epoch,
                code_index: 0,
                mmap_ptr,
                mmap_len: page_size,
            }),
        })
    }

    fn timestamp(epoch: &Instant) -> u64 {
        epoch.elapsed().as_nanos() as u64
    }

    /// Write a JIT_CODE_LOAD record for a compiled function.
    pub fn write_code_load(&self, name: &str, code_addr: u64, code: &[u8]) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let timestamp = Self::timestamp(&inner.epoch);
        let name_len = name.len() + 1; // null terminator
        let total_size = 12 + 32 + name_len + code.len();
        let pid = std::process::id();
        let code_index = inner.code_index;

        let f = &mut inner.file;
        // jr_prefix
        f.write_all(&JIT_CODE_LOAD.to_le_bytes())?;
        f.write_all(&(total_size as u32).to_le_bytes())?;
        f.write_all(&timestamp.to_le_bytes())?;
        // jr_code_load
        f.write_all(&pid.to_le_bytes())?;
        f.write_all(&pid.to_le_bytes())?; // tid = pid
        f.write_all(&code_addr.to_le_bytes())?; // vma
        f.write_all(&code_addr.to_le_bytes())?; // code_addr
        f.write_all(&(code.len() as u64).to_le_bytes())?; // code_size
        f.write_all(&code_index.to_le_bytes())?; // code_index
        // name + null
        f.write_all(name.as_bytes())?;
        f.write_all(&[0u8])?;
        // code bytes
        f.write_all(code)?;
        f.flush()?;
        inner.code_index += 1;
        Ok(())
    }

    /// Write a JIT_CODE_DEBUG_INFO record mapping code offsets to source lines.
    pub fn write_debug_info(&self, code_addr: u64, entries: &[DebugEntry]) -> io::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut inner = self.inner.lock().unwrap();
        let timestamp = Self::timestamp(&inner.epoch);

        let entries_size: usize = entries.iter()
            .map(|e| 8 + 4 + 4 + e.filename.len() + 1)
            .sum();
        let total_size = 12 + 8 + 8 + entries_size;

        let f = &mut inner.file;
        // jr_prefix
        f.write_all(&JIT_CODE_DEBUG_INFO.to_le_bytes())?;
        f.write_all(&(total_size as u32).to_le_bytes())?;
        f.write_all(&timestamp.to_le_bytes())?;
        // jr_code_debug_info
        f.write_all(&code_addr.to_le_bytes())?;
        f.write_all(&(entries.len() as u64).to_le_bytes())?;
        // debug entries
        for entry in entries {
            f.write_all(&entry.code_addr.to_le_bytes())?;
            f.write_all(&(entry.line as i32).to_le_bytes())?;
            f.write_all(&0i32.to_le_bytes())?; // discrim
            f.write_all(entry.filename.as_bytes())?;
            f.write_all(&[0u8])?;
        }
        f.flush()
    }

    /// Write a JIT_CODE_CLOSE record.
    pub fn close(&self) -> io::Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let timestamp = Self::timestamp(&inner.epoch);
        let f = &mut inner.file;
        f.write_all(&JIT_CODE_CLOSE.to_le_bytes())?;
        f.write_all(&12u32.to_le_bytes())?;
        f.write_all(&timestamp.to_le_bytes())?;
        f.flush()
    }
}

impl Drop for JitdumpWriter {
    fn drop(&mut self) {
        let inner = self.inner.lock().unwrap();
        if !inner.mmap_ptr.is_null() && inner.mmap_ptr != ffi::MAP_FAILED {
            unsafe { ffi::munmap(inner.mmap_ptr, inner.mmap_len); }
        }
    }
}
