//! This module implements the MemSpace structure which is responsible for handling the memory
//! mapping of execution contexts.
//!
//! The memory space contains two types of structures:
//! - Mapping: A region of virtual memory that is allocated
//! - Gap: A region of virtual memory that is available to be allocated

mod gap;
mod mapping;
mod physical_ref_counter;

use core::cmp::Ordering;
use core::ffi::c_void;
use core::ptr::NonNull;
use crate::errno::Errno;
use crate::errno;
use crate::memory::stack;
use crate::memory::vmem::VMem;
use crate::memory::vmem;
use crate::memory;
use crate::process::oom;
use crate::util::FailableClone;
use crate::util::boxed::Box;
use crate::util::container::binary_tree::BinaryTree;
use crate::util::lock::mutex::Mutex;
use gap::MemGap;
use mapping::MemMapping;
use physical_ref_counter::PhysRefCounter;

/// Flag telling that a memory mapping can be written to.
pub const MAPPING_FLAG_WRITE: u8  = 0b00001;
/// Flag telling that a memory mapping can contain executable instructions.
pub const MAPPING_FLAG_EXEC: u8   = 0b00010;
/// Flag telling that a memory mapping is accessible from userspace.
pub const MAPPING_FLAG_USER: u8   = 0b00100;
/// Flag telling that a memory mapping must allocate its physical memory right away and not when
/// the process tries to write to it.
pub const MAPPING_FLAG_NOLAZY: u8 = 0b01000;
/// Flag telling that a memory mapping has its physical memory shared with one or more other
/// mappings.
pub const MAPPING_FLAG_SHARED: u8 = 0b10000;

/// The size of the temporary stack used to fork a memory space.
const TMP_STACK_SIZE: usize = memory::PAGE_SIZE * 8;

/// The physical pages reference counter.
pub static mut PHYSICAL_REF_COUNTER: Mutex<PhysRefCounter> = Mutex::new(PhysRefCounter::new());

/// Structure representing the data passed to the temporary stack used to fork a memory space.
/// It is necessary to switch stacks because using a stack while mapping it is undefined.
struct ForkData<'a> {
	/// A reference to the memory space.
	self_: &'a mut MemSpace,

	/// The result of the mapping operation.
	result: Result<MemSpace, Errno>,
}

/// Structure representing the virtual memory space of a context.
pub struct MemSpace {
	/// Binary tree storing the list of memory gaps, ready for new mappings. Sorted by pointer to
	/// the beginning of the mapping on the virtual memory.
	gaps: BinaryTree::<*const c_void, MemGap>,
	/// Binary tree storing the list of memory gaps, sorted by size. The key is the size of the gap
	/// and the value is the pointer to its beginning.
	gaps_size: BinaryTree::<usize, *const c_void>,

	/// Binary tree storing the list of memory mappings. Sorted by pointer to the beginning of the
	/// mapping on the virtual memory.
	mappings: BinaryTree::<*const c_void, MemMapping>,

	/// The virtual memory context handler.
	vmem: Box::<dyn VMem>,
}

impl MemSpace {
	/// Inserts the given gap into the memory space's structures.
	fn gap_insert(&mut self, gap: MemGap) -> Result<(), Errno> {
		let gap_ptr = gap.get_begin();
		let g = self.gaps.insert(gap_ptr, gap)?;
		self.gaps_size.insert(g.get_size(), gap_ptr)?;

		Ok(())
	}

	/// Removes the given gap from the memory space's structures.
	fn gap_remove(&mut self, gap_begin: *const c_void) {
		let g = self.gaps.remove(gap_begin).unwrap();
		self.gaps_size.select_remove(g.get_size(), | val | {
			*val == gap_begin
		});
	}

	/// Returns a reference to a gap with at least size `size`.
	/// `gaps` is the binary tree storing gaps, sorted by pointer to their respective beginnings.
	/// `gaps_size` is the binary tree storing pointers to gaps, sorted by gap sizes.
	/// `size` is the minimum size of the gap.
	/// If no gap large enough is available, the function returns None.
	fn gap_get<'a>(gaps: &'a mut BinaryTree<*const c_void, MemGap>,
		gaps_size: &mut BinaryTree<usize, *const c_void>, size: usize) -> Option<&'a mut MemGap> {
		let ptr = gaps_size.get_min(size)?.1;
		let gap = gaps.get_mut(*ptr).unwrap();
		debug_assert!(gap.get_size() >= size);

		Some(gap)
	}

	/// Returns a new binary tree containing the default gaps for a memory space.
	fn create_default_gaps(&mut self) -> Result::<(), Errno> {
		let begin = memory::ALLOC_BEGIN;
		let size = (memory::PROCESS_END as usize - begin as usize) / memory::PAGE_SIZE;
		self.gap_insert(MemGap::new(begin, size))
	}

	/// Creates a new virtual memory object.
	pub fn new() -> Result::<Self, Errno> {
		let mut s = Self {
			gaps: BinaryTree::new(),
			gaps_size: BinaryTree::new(),

			mappings: BinaryTree::new(),

			vmem: vmem::new()?,
		};
		s.create_default_gaps()?;
		Ok(s)
	}

	/// Returns a mutable reference to the vvirtual memory context.
	pub fn get_vmem(&mut self) -> &mut Box::<dyn VMem> {
		&mut self.vmem
	}

	/// Maps a region of memory.
	/// `ptr` represents the address of the beginning of the region on the virtual memory.
	/// If the address is None, the function shall find a gap in the memory space that is large
	/// enough to contain the mapping.
	/// `size` represents the size of the region in number of memory pages.
	/// `flags` represents the flags for the mapping.
	/// underlying physical memory is not allocated directly but only an attempt to write the
	/// memory is detected.
	/// The function returns a pointer to the newly mapped virtual memory.
	pub fn map(&mut self, ptr: Option::<*const c_void>, size: usize, flags: u8)
		-> Result<*const c_void, Errno> {
		if let Some(_ptr) = ptr {
			// TODO Insert mapping at exact location if possible
			// Err(errno::ENOMEM)
			todo!();
		} else {
			let gap = Self::gap_get(&mut self.gaps, &mut self.gaps_size, size);
			if gap.is_none() {
				return Err(errno::ENOMEM);
			}

			let gap = gap.unwrap();
			let gap_ptr = gap.get_begin();

			let mapping = MemMapping::new(gap_ptr, size, flags,
				NonNull::new(self.vmem.as_mut_ptr()).unwrap());
			let mapping_ptr = mapping.get_begin();
			let m = self.mappings.insert(mapping_ptr, mapping)?;

			if m.map_default().is_err() {
				self.mappings.remove(mapping_ptr);
				return Err(errno::ENOMEM);
			}

			if let Some(new_gap) = gap.consume(size) {
				if self.gap_insert(new_gap).is_err() {
					// TODO Check if the error can happen in this context
					let _ = self.mappings.get_mut(mapping_ptr).unwrap().unmap();
					self.mappings.remove(mapping_ptr);
					return Err(errno::ENOMEM);
				}
			}

			self.gap_remove(gap_ptr);
			Ok(mapping_ptr)
		}
	}

	/// Same as `map`, except the function returns a pointer to the end of the memory region.
	pub fn map_stack(&mut self, ptr: Option::<*const c_void>, size: usize, flags: u8)
		-> Result<*const c_void, Errno> {
		let mapping_ptr = self.map(ptr, size, flags)?;
		Ok(unsafe { // Safe because the new pointer stays in the range of the allocated mapping
			mapping_ptr.add(size * memory::PAGE_SIZE)
		})
	}

	/// Returns a mutable reference to the memory mapping containing the given virtual address
	/// `ptr` from mappings container `mappings`. If no mapping contains the address, the function
	/// returns None.
	fn get_mapping_for(mappings: &mut BinaryTree::<*const c_void, MemMapping>, ptr: *const c_void)
		-> Option::<&mut MemMapping> {
		mappings.cmp_get(| key, value | {
			let begin = *key;
			let end = (begin as usize + value.get_size() * memory::PAGE_SIZE) as *const c_void;
			if ptr >= begin && ptr < end {
				Ordering::Equal
			} else if ptr < begin {
				Ordering::Less
			} else {
				Ordering::Greater
			}
		})
	}

	/// Unmaps the given region of memory.
	/// `ptr` represents the address of the beginning of the region on the virtual memory.
	/// `size` represents the size of the region in number of memory pages.
	/// The function frees the physical memory the region points to unless shared by one or several
	/// other memory mappings.
	/// After this function returns, the access to the region of memory shall be revoked and
	/// further attempts to access it shall result in a page fault.
	pub fn unmap(&mut self, _ptr: *const c_void, _size: usize) {
		// TODO
		todo!();
	}

	/// Tells whether the given region of memory `ptr` of size `size` in bytes can be accessed.
	/// `user` tells whether the memory must be accessible from userspace or just kernelspace.
	/// `write` tells whether to check for write permission.
	pub fn can_access(&self, _ptr: *const u8, _size: usize, _user: bool, _write: bool) -> bool {
		// TODO

		//todo!();
		true
	}

	/// Tells whether the given zero-terminated string beginning at `ptr` can be accessed.
	/// `user` tells whether the memory must be accessible from userspace or just kernelspace.
	/// `write` tells whether to check for write permission.
	/// If the memory cannot be accessed, the function returns None. If it can be accessed, it
	/// returns the length of the string located at the pointer `ptr`.
	pub fn can_access_string(&self, ptr: *const u8, _user: bool, _write: bool) -> Option<usize> {
		// TODO

		//todo!();
		Some(unsafe {
			crate::util::strlen(ptr)
		})
	}

	/// Binds the CPU to this memory space.
	pub fn bind(&self) {
		self.vmem.bind();
	}

	/// Tells whether the memory space is bound.
	pub fn is_bound(&self) -> bool {
		self.vmem.is_bound()
	}

	/// Performs the actions of `fork`. This function is meant to be called onto a temporary stack.
	fn do_fork(&mut self) -> Result<MemSpace, Errno> {
		let mut mem_space = Self {
			gaps: self.gaps.failable_clone()?,
			gaps_size: self.gaps_size.failable_clone()?,

			mappings: BinaryTree::new(),

			vmem: vmem::clone(&self.vmem)?,
		};

		for (_, m) in self.mappings.iter_mut() {
			let new_mapping = m.fork(&mut mem_space)?;

			for i in 0..new_mapping.get_size() {
				m.update_vmem(i);
				new_mapping.update_vmem(i);
			}
		}

		Ok(mem_space)
	}

	/// Clones the current memory space for process forking.
	pub fn fork(&mut self) -> Result<MemSpace, Errno> {
		let tmp_stack = Box::<[u8; TMP_STACK_SIZE]>::new([0; TMP_STACK_SIZE])?;
		let tmp_stack_top = unsafe {
			(tmp_stack.as_ptr() as *mut c_void).add(TMP_STACK_SIZE)
		};

		let f: fn(*mut c_void) -> () = | data: *mut c_void | {
			let data = unsafe {
				&mut *(data as *mut ForkData)
			};
			data.result = data.self_.do_fork();
		};

		unsafe {
			stack::switch(tmp_stack_top, f, ForkData {
				self_: self,

				result: Err(0),
			})?.result
		}
	}

	/// Function called whenever the CPU triggered a page fault for the context. This function
	/// determines whether the process should continue or not. If continuing, the function must
	/// resolve the issue before returning.
	/// A typical situation where is function is usefull is for Copy-On-Write allocations.
	///
	/// `virt_addr` is the virtual address of the wrong memory access that caused the fault.
	/// `code` is the error code given along with the error.
	/// If the process should continue, the function returns `true`, else `false`.
	pub fn handle_page_fault(&mut self, virt_addr: *const c_void, code: u32) -> bool {
		if code & vmem::x86::PAGE_FAULT_PRESENT == 0 {
			return false;
		}

		if let Some(mapping) = Self::get_mapping_for(&mut self.mappings, virt_addr) {
			let offset = (virt_addr as usize - mapping.get_begin() as usize) / memory::PAGE_SIZE;
			if mapping.map(offset).is_err() {
				oom::kill();
				// TODO Check if current process has been killed

				if mapping.map(offset).is_err() {
					crate::kernel_panic!("OOM killer is unable to free up space for new \
allocations!");
				}
			}

			mapping.update_vmem(offset);
			true
		} else {
			false
		}
	}
}
