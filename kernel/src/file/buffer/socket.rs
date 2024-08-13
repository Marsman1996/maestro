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

//! This file implements sockets.

use super::Buffer;
use crate::{
	file::{
		buffer::WaitQueue,
		fs::{Filesystem, NodeOps},
		FileType, INode, Stat,
	},
	net::{osi, SocketDesc, SocketDomain, SocketType},
};
use core::ffi::c_int;
use utils::{
	collections::{ring_buffer::RingBuffer, vec::Vec},
	errno,
	errno::{AllocResult, EResult},
	lock::Mutex,
	ptr::arc::Arc,
	vec, TryDefault,
};

/// The maximum size of a socket's buffers.
const BUFFER_SIZE: usize = 65536;

/// Socket option level: Socket
const SOL_SOCKET: c_int = 1;

/// Structure representing a socket.
#[derive(Debug)]
pub struct Socket {
	/// The socket's stack descriptor.
	desc: SocketDesc,
	/// The socket's network stack corresponding to the descriptor.
	stack: Option<osi::Stack>,

	/// The buffer containing received data. If `None`, reception has been shutdown.
	receive_buffer: Option<RingBuffer<u8, Vec<u8>>>,
	/// The buffer containing data to be transmitted. If `None`, transmission has been shutdown.
	transmit_buffer: Option<RingBuffer<u8, Vec<u8>>>,

	/// The number of entities owning a reference to the socket. When this count reaches zero, the
	/// socket is closed.
	open_count: u32,

	/// The socket's block handler.
	block_handler: WaitQueue,

	/// The address the socket is bound to.
	sockname: Vec<u8>,
}

impl Socket {
	/// Creates a new instance.
	pub fn new(desc: SocketDesc) -> AllocResult<Arc<Mutex<Self>>> {
		Arc::new(Mutex::new(Self {
			desc,
			stack: None,

			receive_buffer: Some(RingBuffer::new(vec![0; BUFFER_SIZE]?)),
			transmit_buffer: Some(RingBuffer::new(vec![0; BUFFER_SIZE]?)),

			open_count: 0,

			block_handler: WaitQueue::default(),

			sockname: Vec::new(),
		}))
	}

	/// Returns the socket's descriptor.
	#[inline(always)]
	pub fn desc(&self) -> &SocketDesc {
		&self.desc
	}

	/// Returns the socket's network stack.
	#[inline(always)]
	pub fn stack(&self) -> Option<&osi::Stack> {
		self.stack.as_ref()
	}

	/// Reads the given socket option.
	///
	/// Arguments:
	/// - `level` is the level (protocol) at which the option is located.
	/// - `optname` is the name of the option.
	pub fn get_opt(&self, _level: c_int, _optname: c_int) -> EResult<&[u8]> {
		// TODO
		todo!()
	}

	/// Writes the given socket option.
	///
	/// Arguments:
	/// - `level` is the level (protocol) at which the option is located.
	/// - `optname` is the name of the option.
	/// - `optval` is the value of the option.
	///
	/// The function returns a value to be returned by the syscall on success.
	pub fn set_opt(&mut self, _level: c_int, _optname: c_int, _optval: &[u8]) -> EResult<c_int> {
		// TODO
		Ok(0)
	}

	/// Returns the name of the socket.
	pub fn get_sockname(&self) -> &[u8] {
		&self.sockname
	}

	/// Tells whether the socket is bound.
	pub fn is_bound(&self) -> bool {
		!self.sockname.is_empty()
	}

	/// Binds the socket to the given address.
	///
	/// `sockaddr` is the new socket name.
	///
	/// If the socket is already bound, or if the address is invalid, or if the address is already
	/// in used, the function returns an error.
	pub fn bind(&mut self, sockaddr: &[u8]) -> EResult<()> {
		if self.is_bound() {
			return Err(errno!(EINVAL));
		}
		// TODO check if address is already in used (EADDRINUSE)
		// TODO check the requested network interface exists (EADDRNOTAVAIL)
		// TODO check address against stack's domain

		self.sockname = Vec::try_from(sockaddr)?;
		Ok(())
	}

	/// Shuts down the receive side of the socket.
	pub fn shutdown_receive(&mut self) {
		self.receive_buffer = None;
	}

	/// Shuts down the transmit side of the socket.
	pub fn shutdown_transmit(&mut self) {
		self.transmit_buffer = None;
	}
}

impl TryDefault for Socket {
	fn try_default() -> Result<Self, Self::Error> {
		let desc = SocketDesc {
			domain: SocketDomain::AfUnix,
			type_: SocketType::SockRaw,
			protocol: 0,
		};

		Ok(Self {
			desc,
			stack: None,

			receive_buffer: Some(RingBuffer::new(vec![0; BUFFER_SIZE]?)),
			transmit_buffer: Some(RingBuffer::new(vec![0; BUFFER_SIZE]?)),

			open_count: 0,

			block_handler: WaitQueue::default(),

			sockname: Default::default(),
		})
	}
}

impl Buffer for Socket {
	fn get_capacity(&self) -> usize {
		// TODO
		todo!()
	}

	fn increment_open(&mut self, _read: bool, _write: bool) {
		self.open_count += 1;
	}

	fn decrement_open(&mut self, _read: bool, _write: bool) {
		self.open_count -= 1;
		if self.open_count == 0 {
			// TODO close the socket
		}
	}
}

impl NodeOps for Socket {
	fn get_stat(&self, _inode: INode, _fs: &dyn Filesystem) -> EResult<Stat> {
		Ok(Stat {
			file_type: FileType::Socket,
			mode: 0o666,
			..Default::default()
		})
	}

	fn read_content(
		&self,
		_inode: INode,
		_fs: &dyn Filesystem,
		_off: u64,
		_buf: &mut [u8],
	) -> EResult<usize> {
		if !self.desc.type_.is_stream() {
			// TODO error
		}
		// TODO
		todo!()
	}

	fn write_content(
		&self,
		_inode: INode,
		_fs: &dyn Filesystem,
		_off: u64,
		_buf: &[u8],
	) -> EResult<usize> {
		// A destination address is required
		let Some(_stack) = self.stack.as_ref() else {
			return Err(errno!(EDESTADDRREQ));
		};
		// TODO
		todo!()
	}
}
