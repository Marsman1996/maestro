//! The `getgid32` syscall returns the GID of the process's owner.

use crate::{errno::Errno, process::Process};
use macros::syscall;

#[syscall]
pub fn getgid32() -> Result<i32, Errno> {
	let proc_mutex = Process::current_assert();
	let proc = proc_mutex.lock();
	Ok(proc.access_profile.get_gid() as _)
}
