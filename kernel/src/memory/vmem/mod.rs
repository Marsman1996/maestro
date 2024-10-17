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

//! The virtual memory makes the kernel able to isolate processes, which is
//! essential for modern systems.

#[cfg(target_arch = "x86")]
pub mod x86;

use crate::{
	cpu, elf, idt, memory,
	memory::{PhysAddr, VirtAddr, KERNELSPACE_SIZE},
	register_get,
	tty::vga,
};
use core::{alloc::AllocError, mem, ptr::NonNull};
use utils::{
	collections::vec::Vec,
	errno::AllocResult,
	limits::PAGE_SIZE,
	lock::{once::OnceInit, Mutex},
	vec,
};

/// Tells whether the given range of memory overlaps with the kernelspace.
///
/// Arguments:
/// - `virtaddr` is the start of the range.
/// - `pages` is the size of the range in pages.
fn is_kernelspace(virtaddr: VirtAddr, pages: usize) -> bool {
	let Some(end) = virtaddr.0.checked_add(pages * PAGE_SIZE) else {
		return true;
	};
	end > memory::PROCESS_END.0
}

/// A virtual memory context.
///
/// This structure implements operations to modify virtual memory in an architecture-independent
/// way.
///
/// `KERNEL` specifies whether mapping in kernelspace is allowed. If not allowed, trying to do it
/// results in an error.
pub struct VMem<const KERNEL: bool = false> {
	#[cfg(target_arch = "x86")]
	page_dir: NonNull<x86::Table>,
}

impl VMem<false> {
	/// Creates a new virtual memory context.
	pub fn new() -> AllocResult<Self> {
		Ok(Self {
			#[cfg(target_arch = "x86")]
			page_dir: x86::alloc()?,
		})
	}
}

impl VMem<true> {
	/// Creates a new virtual memory context which is allowed to modify kernelspace.
	///
	/// # Safety
	///
	/// The caller must ensure that modifying kernelspace keeps the code and stack accessible and
	/// valid. Failure to do so results in an undefined behaviour.
	pub unsafe fn new_kernel() -> AllocResult<Self> {
		Ok(Self {
			#[cfg(target_arch = "x86")]
			page_dir: x86::alloc()?,
		})
	}
}

impl<const KERNEL: bool> VMem<KERNEL> {
	/// Returns an immutable reference to the **architecture-dependent** inner representation.
	#[cfg(target_arch = "x86")]
	pub fn inner(&self) -> &x86::Table {
		unsafe { self.page_dir.as_ref() }
	}

	/// Returns a mutable reference to the architecture-dependent inner representation.
	#[cfg(target_arch = "x86")]
	pub fn inner_mut(&mut self) -> &mut x86::Table {
		unsafe { self.page_dir.as_mut() }
	}

	/// Translates the given virtual address `addr` to the corresponding physical
	/// address.
	///
	/// If the address is not mapped, the function returns `None`.
	pub fn translate(&self, addr: VirtAddr) -> Option<PhysAddr> {
		#[cfg(target_arch = "x86")]
		x86::translate(self.inner(), addr)
	}

	/// Begins a transaction.
	pub fn transaction(&mut self) -> VMemTransaction<'_, KERNEL> {
		VMemTransaction {
			vmem: self,
			rollback: vec![],
		}
	}

	/// Binds the virtual memory context to the current CPU.
	pub fn bind(&self) {
		let phys_addr = VirtAddr::from(self.page_dir.as_ptr())
			.kernel_to_physical()
			.unwrap();
		unsafe {
			#[cfg(target_arch = "x86")]
			x86::bind(phys_addr);
		}
	}

	/// Tells whether the context is bound to the current CPU.
	pub fn is_bound(&self) -> bool {
		x86::is_bound(self.page_dir)
	}
}

impl<const KERNEL: bool> Drop for VMem<KERNEL> {
	fn drop(&mut self) {
		if self.is_bound() {
			panic!("Dropping virtual memory context while in use!");
		}
		#[cfg(target_arch = "x86")]
		unsafe {
			x86::free(self.page_dir);
		}
	}
}

/// Handle allowing to roll back operations on a virtual memory context.
///
/// Dropping the transaction without committing rollbacks all modifications.
#[must_use = "A vmem transaction has to be committed or explicitly ignored"]
pub struct VMemTransaction<'v, const KERNEL: bool> {
	/// The virtual memory context on which the transaction applies.
	pub vmem: &'v mut VMem<KERNEL>,
	/// The vector of handles to roll back the whole transaction.
	#[cfg(target_arch = "x86")]
	rollback: Vec<x86::Rollback>,
}

impl<'v, const KERNEL: bool> VMemTransaction<'v, KERNEL> {
	#[cfg(target_arch = "x86")]
	fn map_impl(
		&mut self,
		physaddr: PhysAddr,
		virtaddr: VirtAddr,
		flags: u32,
	) -> AllocResult<x86::Rollback> {
		let res = unsafe { x86::map(self.vmem.inner_mut(), physaddr, virtaddr, flags) };
		invalidate_page_current(virtaddr);
		res
	}

	/// Maps a single page of virtual memory at `virtaddr` to a single page of physical memory at
	/// `physaddr`.
	///
	/// `flags` is the set of flags to use for the mapping, which are architecture-dependent.
	///
	/// The modifications may not be flushed to the cache. It is the caller's responsibility to
	/// ensure they are.
	#[inline]
	pub fn map(&mut self, physaddr: PhysAddr, virtaddr: VirtAddr, flags: u32) -> AllocResult<()> {
		// If kernelspace modification is disabled, error if mapping onto kernelspace
		if !KERNEL && is_kernelspace(virtaddr, 1) {
			return Err(AllocError);
		}
		let r = self.map_impl(physaddr, virtaddr, flags)?;
		self.rollback.push(r)
	}

	/// Like [`Self::map`] but on a range of several pages.
	///
	/// On overflow, the physical and virtual addresses wrap around the userspace.
	pub fn map_range(
		&mut self,
		physaddr: PhysAddr,
		virtaddr: VirtAddr,
		pages: usize,
		flags: u32,
	) -> AllocResult<()> {
		if pages == 0 {
			// No op
			return Ok(());
		}
		if pages == 1 {
			return self.map(physaddr, virtaddr, flags);
		}
		// If kernelspace modification is disabled, error if mapping onto kernelspace
		if !KERNEL && is_kernelspace(virtaddr, pages) {
			return Err(AllocError);
		}
		// Map each page
		self.rollback.reserve(pages)?;
		for i in 0..pages {
			let physaddr = physaddr + i * PAGE_SIZE;
			let virtaddr = virtaddr + i * PAGE_SIZE;
			let r = self.map_impl(physaddr, virtaddr, flags)?;
			self.rollback.push(r)?;
		}
		Ok(())
	}

	#[cfg(target_arch = "x86")]
	fn unmap_impl(&mut self, virtaddr: VirtAddr) -> AllocResult<x86::Rollback> {
		let res = unsafe { x86::unmap(self.vmem.inner_mut(), virtaddr) };
		invalidate_page_current(virtaddr);
		res
	}

	/// Unmaps a single page of virtual memory at `virtaddr`.
	///
	/// The modifications may not be flushed to the cache. It is the caller's responsibility to
	/// ensure they are.
	#[inline]
	pub fn unmap(&mut self, virtaddr: VirtAddr) -> AllocResult<()> {
		// If kernelspace modification is disabled, error if unmapping onto kernelspace
		if !KERNEL && is_kernelspace(virtaddr, 1) {
			return Err(AllocError);
		}
		let r = self.unmap_impl(virtaddr)?;
		self.rollback.push(r)
	}

	/// Like [`Self::unmap`] but on a range of several pages.
	///
	/// On overflow, the physical and virtual addresses wrap around the userspace.
	pub fn unmap_range(&mut self, virtaddr: VirtAddr, pages: usize) -> AllocResult<()> {
		if pages == 0 {
			// No op
			return Ok(());
		}
		if pages == 1 {
			return self.unmap(virtaddr);
		}
		// If kernelspace modification is disabled, error if unmapping onto kernelspace
		if !KERNEL && is_kernelspace(virtaddr, pages) {
			return Err(AllocError);
		}
		// Map each page
		self.rollback.reserve(pages)?;
		for i in 0..pages {
			let virtaddr = virtaddr + i * PAGE_SIZE;
			let r = self.unmap_impl(virtaddr)?;
			self.rollback.push(r)?;
		}
		Ok(())
	}

	/// Validates the transaction.
	pub fn commit(&mut self) {
		self.rollback.clear();
	}
}

impl<const KERNEL: bool> Drop for VMemTransaction<'_, KERNEL> {
	fn drop(&mut self) {
		let rollback = mem::take(&mut self.rollback);
		// Rollback in reverse order
		rollback
			.into_iter()
			.rev()
			.for_each(|r| r.rollback(self.vmem.inner_mut()));
	}
}

/// Invalidate the page at the given address on the current CPU.
pub fn invalidate_page_current(addr: VirtAddr) {
	#[cfg(target_arch = "x86")]
	x86::invalidate_page_current(addr);
}

/// Flush the Translation Lookaside Buffer (TLB) on the current CPU.
///
/// This function should be called after applying modifications to the context for them to be
/// taken into account.
///
/// This is an expensive operation for the CPU cache and should be used as few as possible.
pub fn flush_current() {
	#[cfg(target_arch = "x86")]
	x86::flush_current();
}

/// Executes the closure while allowing the kernel to write on read-only pages.
///
/// # Safety
///
/// This function disables memory protection on the kernel side, which makes
/// read-only data writable.
///
/// Writing on read-only regions of memory has an undefined behavior.
#[inline]
pub unsafe fn write_ro<F: FnOnce() -> T, T>(f: F) -> T {
	cpu::set_write_protected(false);
	let res = f();
	cpu::set_write_protected(true);
	res
}

/// Executes the closure while allowing the kernel to access user data by disabling SMAP.
///
/// # Safety
///
/// SMAP provides a security against potentially malicious data accesses. As such, it should be
/// disabled only when strictly necessary.
///
/// Enabling SMAP removes access to memory addresses that were previously accessible. It is the
/// caller's responsibility to ensure no invalid memory accesses are done afterward.
#[inline]
pub unsafe fn smap_disable<F: FnOnce() -> T, T>(f: F) -> T {
	cpu::set_smap_enabled(false);
	let res = f();
	cpu::set_smap_enabled(true);
	res
}

/// Executes the given closure `f` while being bound to the given virtual memory
/// context `vmem`.
///
/// After execution, the function restores the previous context.
///
/// The function disables interruptions while executing the closure. This is due
/// to the fact that if interruptions were enabled, the scheduler would be able
/// to change the running process, and thus when resuming execution, the virtual
/// memory context would be changed to the process's context, making the
/// behaviour undefined.
///
/// # Safety
///
/// The caller must ensure that the stack is accessible in both the current and given virtual
/// memory contexts.
pub unsafe fn switch<F: FnOnce() -> T, T>(vmem: &VMem, f: F) -> T {
	idt::wrap_disable_interrupts(|| {
		if vmem.is_bound() {
			f()
		} else {
			// Get current vmem
			let page_dir = PhysAddr(register_get!("cr3"));
			// Bind temporary vmem
			vmem.bind();
			let result = f();
			// Restore previous vmem
			x86::bind(page_dir);
			result
		}
	})
}

/// The kernel's virtual memory context.
static KERNEL_VMEM: OnceInit<Mutex<VMem<true>>> = unsafe { OnceInit::new() };

/// Returns a reference to the kernel's virtual memory context.
pub fn kernel() -> &'static Mutex<VMem<true>> {
	KERNEL_VMEM.get()
}

/// Initializes virtual memory management.
pub(crate) fn init() -> AllocResult<()> {
	// Architecture-specific init
	#[cfg(target_arch = "x86")]
	{
		x86::init()?;
	}
	// Kernel context init
	let mut kernel_vmem = unsafe { VMem::new_kernel()? };
	let mut transaction = kernel_vmem.transaction();
	// TODO If Meltdown mitigation is enabled, only allow read access to a stub of
	// the kernel for interrupts
	// Map kernel
	transaction.map_range(
		PhysAddr::default(),
		memory::PROCESS_END,
		KERNELSPACE_SIZE / PAGE_SIZE,
		x86::FLAG_WRITE | x86::FLAG_GLOBAL,
	)?;
	// Make the kernel's code read-only
	let iter = elf::kernel::sections().filter(|s| s.sh_addralign as usize == PAGE_SIZE);
	for section in iter {
		let write = section.sh_flags & elf::SHF_WRITE != 0;
		let user = elf::kernel::get_section_name(section) == Some(b".user");
		let mut flags = x86::FLAG_GLOBAL;
		if write {
			flags |= x86::FLAG_WRITE;
		}
		if user {
			flags |= x86::FLAG_USER;
		}
		// Map
		let virt_addr = VirtAddr(section.sh_addr as _);
		let Some(phys_addr) = virt_addr.kernel_to_physical() else {
			continue;
		};
		let pages = section.sh_size.div_ceil(PAGE_SIZE as _) as usize;
		transaction.map_range(phys_addr, virt_addr, pages, flags)?;
	}
	// Map VGA buffer
	#[cfg(target_arch = "x86")]
	{
		transaction.map_range(
			vga::BUFFER_PHYS as _,
			vga::get_buffer_virt().into(),
			1,
			x86::FLAG_CACHE_DISABLE | x86::FLAG_WRITE_THROUGH | x86::FLAG_WRITE | x86::FLAG_GLOBAL,
		)?;
	}
	transaction.commit();
	drop(transaction);
	kernel_vmem.bind();
	unsafe {
		KERNEL_VMEM.init(Mutex::new(kernel_vmem));
	}
	Ok(())
}

#[cfg(test)]
mod test {
	use super::*;

	#[test_case]
	fn vmem_basic0() {
		let vmem = VMem::new().unwrap();
		for i in (0usize..0xc0000000).step_by(PAGE_SIZE) {
			assert_eq!(vmem.translate(VirtAddr(i)), None);
		}
	}

	#[test_case]
	fn vmem_basic1() {
		let vmem = VMem::new().unwrap();
		for i in (0..0x40000000).step_by(PAGE_SIZE) {
			assert_eq!(vmem.translate(memory::PROCESS_END + i), Some(PhysAddr(i)));
		}
	}

	#[test_case]
	fn vmem_map0() {
		let mut vmem = VMem::new().unwrap();
		let mut transaction = vmem.transaction();
		transaction
			.map(PhysAddr(0x100000), VirtAddr(0x100000), 0)
			.unwrap();
		transaction.commit();
		drop(transaction);
		for i in (0usize..0xc0000000).step_by(PAGE_SIZE) {
			let res = vmem.translate(VirtAddr(i));
			if (0x100000..0x101000).contains(&i) {
				assert_eq!(res, Some(PhysAddr(i)));
			} else {
				assert_eq!(res, None);
			}
		}
	}

	#[test_case]
	fn vmem_map1() {
		let mut vmem = VMem::new().unwrap();
		let mut transaction = vmem.transaction();
		transaction
			.map(PhysAddr(0x100000), VirtAddr(0x100000), 0)
			.unwrap();
		transaction
			.map(PhysAddr(0x200000), VirtAddr(0x100000), 0)
			.unwrap();
		transaction.commit();
		drop(transaction);
		for i in (0usize..0xc0000000).step_by(PAGE_SIZE) {
			let res = vmem.translate(VirtAddr(i));
			if (0x100000..0x101000).contains(&i) {
				assert_eq!(res, Some(PhysAddr(0x100000 + i)));
			} else {
				assert_eq!(res, None);
			}
		}
	}

	#[test_case]
	fn vmem_unmap0() {
		let mut vmem = VMem::new().unwrap();
		let mut transaction = vmem.transaction();
		transaction
			.map(PhysAddr(0x100000), VirtAddr(0x100000), 0)
			.unwrap();
		transaction.unmap(VirtAddr(0x100000)).unwrap();
		transaction.commit();
		drop(transaction);
		for i in (0usize..0xc0000000).step_by(PAGE_SIZE) {
			assert_eq!(vmem.translate(VirtAddr(i)), None);
		}
	}
}