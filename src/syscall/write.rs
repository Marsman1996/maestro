//! This module implements the `write` system call, which allows to write data to a file.

use core::cmp::min;
use crate::errno::Errno;
use crate::errno;
use crate::file::open_file::O_NONBLOCK;
use crate::process::Process;
use crate::process::mem_space::ptr::SyscallSlice;
use crate::process::regs::Regs;

// TODO Return EPIPE and kill with SIGPIPE when writing on a broken pipe
// TODO O_ASYNC

/// The implementation of the `write` syscall.
pub fn write(regs: &Regs) -> Result<i32, Errno> {
	let fd = regs.ebx;
	let buf: SyscallSlice<u8> = (regs.ecx as usize).into();
	let count = regs.edx as usize;

	let len = min(count, i32::MAX as usize);
	if len == 0 {
		return Ok(0);
	}

	loop {
		// Trying to write and getting the length of written data
		let (len, flags) = {
			let mutex = Process::get_current().unwrap();
			let mut guard = mutex.lock();
			let proc = guard.get_mut();

			let mem_space = proc.get_mem_space().unwrap();
			let mem_space_guard = mem_space.lock();
			let buf_slice = buf.get(&mem_space_guard, len)?.ok_or(errno!(EFAULT))?;

			let open_file_mutex = proc.get_open_file(fd).ok_or(errno!(EBADF))?;
			let mut open_file_guard = open_file_mutex.lock();
			let open_file = open_file_guard.get_mut();

			let flags = open_file.get_flags();
			(open_file.write(buf_slice)?, flags) // TODO On EPIPE, kill current with SIGPIPE
		};

		// TODO Continue until everything was written?
		// If the length is greater than zero, success
		if len > 0 {
			return Ok(len as _);
		}

		if flags & O_NONBLOCK != 0 {
			// The file descriptor is non blocking
			return Err(errno!(EAGAIN));
		}

		// TODO Mark the process as Sleeping and wake it up when data can be written?
		crate::wait();
	}
}
