//! This module implements the `exe` node, which is a link to the executable
//! file of the process.

use crate::{
	errno::{EResult, Errno},
	file::{
		fs::kernfs::{content::KernFSContent, node::KernFSNode},
		perm::{Gid, Uid},
		FileContent, Mode,
	},
	process::{pid::Pid, Process},
	util::{io::IO, TryClone},
};

/// Structure representing the `exe` node.
#[derive(Debug)]
pub struct Exe {
	/// The PID of the process.
	pub pid: Pid,
}

impl KernFSNode for Exe {
	fn get_mode(&self) -> Mode {
		0o777
	}

	fn get_uid(&self) -> Uid {
		if let Some(proc_mutex) = Process::get_by_pid(self.pid) {
			proc_mutex.lock().access_profile.get_euid()
		} else {
			0
		}
	}

	fn get_gid(&self) -> Gid {
		if let Some(proc_mutex) = Process::get_by_pid(self.pid) {
			proc_mutex.lock().access_profile.get_egid()
		} else {
			0
		}
	}

	fn get_content(&mut self) -> EResult<KernFSContent<'_>> {
		let content = Process::get_by_pid(self.pid)
			.map(|mutex| {
				let proc = mutex.lock();
				(*proc.exec_path).try_clone()
			})
			.transpose()?
			.unwrap_or_default();
		Ok(FileContent::Link(content).into())
	}
}

impl IO for Exe {
	fn get_size(&self) -> u64 {
		0
	}

	fn read(&mut self, _offset: u64, _buff: &mut [u8]) -> Result<(u64, bool), Errno> {
		Err(errno!(EINVAL))
	}

	fn write(&mut self, _offset: u64, _buff: &[u8]) -> Result<u64, Errno> {
		Err(errno!(EINVAL))
	}

	fn poll(&mut self, _mask: u32) -> Result<u32, Errno> {
		Err(errno!(EINVAL))
	}
}
