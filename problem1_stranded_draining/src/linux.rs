use std::env;
use std::fs;
use std::io;
use std::mem;
use std::path::{Path, PathBuf};

use crate::protocol::CpuId;

pub const SCHED_EXT: i32 = 7;

pub fn set_current_affinity(cpus: &[CpuId]) -> io::Result<()> {
    if cpus.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "affinity requires at least one CPU",
        ));
    }

    unsafe {
        let mut set: libc::cpu_set_t = mem::zeroed();
        libc::CPU_ZERO(&mut set);
        for cpu in cpus {
            libc::CPU_SET(*cpu as usize, &mut set);
        }

        let ret = libc::sched_setaffinity(0, mem::size_of::<libc::cpu_set_t>(), &set);
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

pub fn set_current_sched_ext() -> io::Result<()> {
    unsafe {
        let param = libc::sched_param { sched_priority: 0 };
        let ret = libc::sched_setscheduler(0, SCHED_EXT, &param);
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

pub fn effective_uid() -> io::Result<u32> {
    let status = fs::read_to_string("/proc/self/status")?;
    for line in status.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.first().copied() == Some("Uid:") && fields.len() >= 3 {
            return fields[2].parse::<u32>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid effective uid in /proc/self/status: {error}"),
                )
            });
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Uid line not found in /proc/self/status",
    ))
}

pub fn find_in_path(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return is_executable(&path).then_some(path);
    }

    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}

pub fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(tmp, path)
}
