//! A filesystem is the representation of the file hierarchy on a storage device.

pub mod ext2;

use crate::device::Device;
use crate::device::DeviceHandle;
use crate::errno::Errno;
use crate::errno;
use crate::util::boxed::Box;
use crate::util::container::vec::Vec;
use crate::util::lock::mutex::Mutex;
use crate::util::lock::mutex::MutexGuard;
use crate::util::ptr::SharedPtr;
use super::File;
use super::INode;
use super::path::Path;

/// Trait representing a filesystem.
pub trait Filesystem {
	/// Returns the name of the filesystem.
	fn get_name(&self) -> &str;

	/// Tells whether the filesystem is mounted in read-only.
	fn is_readonly(&self) -> bool;

	/// Loads the file at path `path`.
	/// `io` is the I/O interface.
	/// `path` is the file's path.
	/// The path must be absolute relative the filesystem's root directory and must not contain
	/// any `.` or `..` component.
	fn load_file(&mut self, io: &mut dyn DeviceHandle, path: Path) -> Result<File, Errno>;
	/// Adds a file to the filesystem.
	/// `io` is the I/O interface.
	/// `path` is the parent directory's path.
	/// `file` is the file to be added.
	/// The path must be absolute relative the filesystem's root directory and must not contain
	/// any `.` or `..` component.
	fn add_file(&mut self, io: &mut dyn DeviceHandle, path: Path, file: File) -> Result<(), Errno>;
	/// Removes a file from the filesystem.
	/// `io` is the I/O interface.
	/// `path` is the file's path.
	/// The path must be absolute relative the filesystem's root directory and must not contain
	/// any `.` or `..` component.
	fn remove_file(&mut self, io: &mut dyn DeviceHandle, path: Path) -> Result<(), Errno>;

	/// Reads from the given node `node` into the buffer `buf`.
	/// The path must be absolute relative the filesystem's root directory and must not contain
	/// any `.` or `..` component.
	fn read_node(&mut self, io: &mut dyn DeviceHandle, node: INode, buf: &mut [u8])
		-> Result<(), Errno>;
	/// Writes to the given node `node` from the buffer `buf`.
	/// The path must be absolute relative the filesystem's root directory and must not contain
	/// any `.` or `..` component.
	fn write_node(&mut self, io: &mut dyn DeviceHandle, node: INode, buf: &[u8])
		-> Result<(), Errno>;
}

/// Trait representing a filesystem type.
pub trait FilesystemType {
	/// Returns the name of the filesystem.
	fn get_name(&self) -> &str;

	/// Tells whether the given device has the current filesystem.
	fn detect(&self, io: &mut dyn DeviceHandle) -> bool;

	/// Creates a new filesystem on the device and returns its instance.
	fn create_filesystem(&self, io: &mut dyn DeviceHandle) -> Result<Box<dyn Filesystem>, Errno>;

	/// Creates a new instance of the filesystem.
	fn load_filesystem(&self, io: &mut dyn DeviceHandle) -> Result<Box<dyn Filesystem>, Errno>;
}

/// The list of mountpoints.
static mut FILESYSTEMS: Mutex<Vec<SharedPtr<dyn FilesystemType>>> = Mutex::new(Vec::new());

/// Registers a new filesystem type `fs`.
pub fn register<T: 'static + FilesystemType>(fs_type: T) -> Result<(), Errno> {
	let mutex = unsafe { // Safe because using Mutex
		&mut FILESYSTEMS
	};
	let mut guard = MutexGuard::new(mutex);
	let container = guard.get_mut();
	container.push(SharedPtr::new(fs_type)?)
}

// TODO Function to unregister a filesystem type

/// Detects the filesystem type on the given device `device`.
pub fn detect(device: &mut Device) -> Result<SharedPtr<dyn FilesystemType>, Errno> {
	let mutex = unsafe { // Safe because using Mutex
		&mut FILESYSTEMS
	};
	let mut guard = MutexGuard::new(mutex);
	let container = guard.get_mut();

	for fs_type in container.iter() {
		if fs_type.detect(device.get_handle()) {
			return Ok(fs_type.clone()); // TODO Use a weak pointer?
		}
	}

	Err(errno::ENODEV)
}

/// Registers the filesystems that are implemented inside of the kernel itself.
/// This function must be called only once, at initialization.
pub fn register_defaults() -> Result<(), Errno> {
	register(ext2::Ext2FsType {})?;

	Ok(())
}
