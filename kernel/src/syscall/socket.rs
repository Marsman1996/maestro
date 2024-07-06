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

//! The `socket` system call allows to create a socket.

use crate::{
	file::{buffer, buffer::socket::Socket, open_file, open_file::OpenFile, vfs},
	net::{SocketDesc, SocketDomain, SocketType},
	process::Process,
	syscall::Args,
};
use core::ffi::c_int;
use utils::{
	errno,
	errno::{EResult, Errno},
};

pub fn socket(Args((domain, r#type, protocol)): Args<(c_int, c_int, c_int)>) -> EResult<usize> {
	let proc_mutex = Process::current_assert();
	let proc = proc_mutex.lock();

	let sock_domain = SocketDomain::try_from(domain as u32)?;
	let sock_type = SocketType::try_from(r#type as u32)?;
	if !proc.access_profile.can_use_sock_domain(&sock_domain)
		|| !proc.access_profile.can_use_sock_type(&sock_type)
	{
		return Err(errno!(EACCES));
	}
	let desc = SocketDesc {
		domain: sock_domain,
		type_: sock_type,
		protocol,
	};

	let sock = Socket::new(desc)?;

	// Get file
	let loc = buffer::register(None, sock)?;
	let file = vfs::get_file_from_location(loc)?;

	let open_file = OpenFile::new(file, None, open_file::O_RDWR)?;

	let fds_mutex = proc.file_descriptors.as_ref().unwrap();
	let mut fds = fds_mutex.lock();
	let (sock_fd_id, _) = fds.create_fd(0, open_file)?;

	Ok(sock_fd_id as _)
}
