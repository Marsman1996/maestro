//! The `close` system call closes the given file descriptor.

use crate::{errno, errno::Errno, process::Process};
use core::ffi::c_int;
use macros::syscall;

#[syscall]
pub fn close(fd: c_int) -> Result<i32, Errno> {
	if fd < 0 {
		return Err(errno!(EBADF));
	}

	let proc_mutex = Process::current_assert();
	let proc = proc_mutex.lock();

	let fds_mutex = proc.file_descriptors.as_ref().unwrap();
	let mut fds = fds_mutex.lock();

	fds.close_fd(fd as _)?;
	Ok(0)
}
