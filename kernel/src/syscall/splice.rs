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

//! The `splice` system call splice data from one pipe to another.

use crate::{
	file::FileType,
	process::{mem_space::copy::SyscallPtr, Process},
	syscall::Args,
};
use core::{
	cmp::min,
	ffi::{c_int, c_uint},
};
use utils::{
	errno,
	errno::{EResult, Errno},
	vec,
};

#[allow(clippy::type_complexity)]
pub fn splice(
	Args((fd_in, off_in, fd_out, off_out, len, _flags)): Args<(
		c_int,
		SyscallPtr<u64>,
		c_int,
		SyscallPtr<u64>,
		usize,
		c_uint,
	)>,
) -> EResult<usize> {
	let (input_mutex, off_in, output_mutex, off_out) = {
		let proc_mutex = Process::current();
		let proc = proc_mutex.lock();

		let fds_mutex = proc.file_descriptors.as_ref().unwrap();
		let fds = fds_mutex.lock();

		let input = fds.get_fd(fd_in)?.get_file().clone();
		let output = fds.get_fd(fd_out)?.get_file().clone();

		let off_in = off_in.copy_from_user()?;
		let off_out = off_out.copy_from_user()?;

		(input, off_in, output, off_out)
	};

	{
		let input_type = input_mutex.lock().get_type()?;
		let output_type = output_mutex.lock().get_type()?;

		let in_is_pipe = matches!(input_type, FileType::Fifo);
		let out_is_pipe = matches!(output_type, FileType::Fifo);

		if !in_is_pipe && !out_is_pipe {
			return Err(errno!(EINVAL));
		}
		if in_is_pipe && off_in.is_some() {
			return Err(errno!(ESPIPE));
		}
		if out_is_pipe && off_out.is_some() {
			return Err(errno!(ESPIPE));
		}
	}

	let len = min(len, i32::MAX as usize);

	// TODO implement flags

	let mut buf = vec![0u8; len]?;

	let len = {
		let mut input = input_mutex.lock();
		let len = input
			.ops()
			.read_content(input.get_location(), off_in.unwrap_or(0), &mut buf)?;
		if off_in.is_none() {
			input.off += len as u64;
		}
		len
	};

	let in_slice = &buf[..len];
	let mut i = 0;
	while i < len {
		// TODO Check for signal (and handle syscall restart correctly with offsets)
		let mut output = output_mutex.lock();
		let l =
			output
				.ops()
				.write_content(output.get_location(), off_out.unwrap_or(0), in_slice)?;
		if off_out.is_none() {
			output.off += l as u64;
		}
		i += l;
	}

	Ok(len as _)
}
