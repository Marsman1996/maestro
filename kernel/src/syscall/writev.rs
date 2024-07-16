/*
 * Copyright 2024 Luc Lenôtre
 *
 * This file is part of Maestro.
 *
 * Maestro is free software: you can redistribute it and/or modify it under the
 * terms of the GNU General Public License as published by the Free Software
 * Foundation, either version 3 of the License, or (at your option) any later
 * version.
 *
 * Maestro is distributed in the hope that it will be useful, but WITHOUT ANY
 * WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR
 * A PARTICULAR PURPOSE. See the GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along with
 * Maestro. If not, see <https://www.gnu.org/licenses/>.
 */

//! The `writev` system call allows to write sparse data on a file descriptor.

use crate::{
	file::{
		fd::FileDescriptorTable,
		open_file::{OpenFile, O_NONBLOCK},
		FileType,
	},
	limits,
	process::{
		iovec::IOVec,
		mem_space::{copy::SyscallSlice, MemSpace},
		scheduler,
		signal::Signal,
		Process,
	},
	syscall::{Args, FromSyscallArg},
};
use core::{cmp::min, ffi::c_int};
use utils::{
	errno,
	errno::{EResult, Errno},
	io,
	io::IO,
	lock::{IntMutex, Mutex},
	ptr::arc::Arc,
};
// TODO Handle blocking writes (and thus, EINTR)

/// Writes the given chunks to the file.
///
/// Arguments:
/// - `iov` is the set of chunks
/// - `iovcnt` is the number of chunks in `iov`
/// - `open_file` is the file to write to
fn write(iov: &SyscallSlice<IOVec>, iovcnt: usize, open_file: &mut OpenFile) -> EResult<i32> {
	let mut off = 0;
	let iov = iov.copy_from_user(..iovcnt)?.ok_or(errno!(EFAULT))?;
	for i in iov {
		// Ignore zero entry
		if i.iov_len == 0 {
			continue;
		}
		// The size to write. This is limited to avoid an overflow on the total length
		let l = min(i.iov_len, usize::MAX - off);
		let ptr = SyscallSlice::<u8>::from_syscall_arg(i.iov_base as usize);
		if let Some(buffer) = ptr.copy_from_user(..l)? {
			// FIXME: if not everything has been written, must retry with the same buffer with the
			// corresponding offset
			off += open_file.write(0, &buffer)? as usize;
		}
	}
	Ok(off as _)
}

/// Performs the `writev` operation.
///
/// Arguments:
/// - `fd` is the file descriptor
/// - `iov` the IO vector
/// - `iovcnt` the number of entries in the IO vector
/// - `offset` is the offset in the file
/// - `flags` is the set of flags
pub fn do_writev(
	fd: i32,
	iov: SyscallSlice<IOVec>,
	iovcnt: i32,
	offset: Option<isize>,
	_flags: Option<i32>,
	fds: &Mutex<FileDescriptorTable>,
	proc: &IntMutex<Process>,
) -> EResult<usize> {
	// Validation
	if iovcnt < 0 || iovcnt as usize > limits::IOV_MAX {
		return Err(errno!(EINVAL));
	}
	let open_file_mutex = fds.lock().get_fd(fd)?.get_open_file().clone();
	// Validation
	let (start_off, update_off) = match offset {
		Some(o @ 0..) => (o as u64, false),
		None | Some(-1) => {
			let open_file = open_file_mutex.lock();
			(open_file.get_offset(), true)
		}
		Some(..-1) => return Err(errno!(EINVAL)),
	};
	let file_type = open_file_mutex.lock().get_file().lock().stat.file_type;
	if file_type == FileType::Link {
		return Err(errno!(EINVAL));
	}
	loop {
		// TODO super::util::signal_check(regs);
		{
			let mut open_file = open_file_mutex.lock();
			let flags = open_file.get_flags();
			// Change the offset temporarily
			let prev_off = open_file.get_offset();
			open_file.set_offset(start_off);
			let len = match write(&iov, iovcnt as _, &mut open_file) {
				Ok(len) => len,
				Err(e) => {
					// If writing to a broken pipe, kill with SIGPIPE
					if e.as_int() == errno::EPIPE {
						let mut proc = proc.lock();
						proc.kill_now(&Signal::SIGPIPE);
					}
					return Err(e);
				}
			};
			// Restore previous offset
			if !update_off {
				open_file.set_offset(prev_off);
			}
			if len > 0 {
				return Ok(len as _);
			}
			if flags & O_NONBLOCK != 0 {
				// The file descriptor is non-blocking
				return Err(errno!(EAGAIN));
			}
			// Block on file
			let mut proc = proc.lock();
			open_file.add_waiting_process(&mut proc, io::POLLOUT | io::POLLERR)?;
		}
		// Make current process sleep
		scheduler::end_tick();
	}
}

pub fn writev(
	Args((fd, iov, iovcnt)): Args<(c_int, SyscallSlice<IOVec>, c_int)>,
	fds: Arc<Mutex<FileDescriptorTable>>,
	proc: &IntMutex<Process>,
) -> EResult<usize> {
	do_writev(fd, iov, iovcnt, None, None, &fds, proc)
}
