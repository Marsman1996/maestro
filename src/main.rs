#![no_std]
#![no_main]

#![feature(allow_internal_unstable)]
#![feature(asm)]
#![feature(const_fn)]
#![feature(const_in_array_repeat_expressions)]
#![feature(const_ptr_offset)]
#![feature(const_raw_ptr_to_usize_cast)]
#![feature(custom_test_frameworks)]
#![feature(intrinsics)]
#![feature(lang_items)]
#![feature(llvm_asm)]
#![feature(maybe_uninit_ref)]
#![feature(panic_info_message)]
#![feature(rustc_attrs)]
#![feature(rustc_private)]
#![feature(untagged_unions)]

#![deny(warnings)]
#![allow(dead_code)]
#![allow(unused_macros)]

#![test_runner(crate::selftest::runner)]
#![reexport_test_harness_main = "test_main"]

mod debug;
mod elf;
mod error;
#[macro_use]
mod idt;
mod memory;
mod multiboot;
#[macro_use]
mod panic;
mod pit;
mod selftest;
mod tty;
#[macro_use]
mod util;
mod vga;

use core::ffi::c_void;
use core::panic::PanicInfo;

/*
 * Current kernel version.
 */
const KERNEL_VERSION: &'static str = "1.0";

extern "C" {
	pub fn kernel_wait();
	pub fn kernel_loop() -> !;
	pub fn kernel_halt() -> !;
}

mod io {
	extern "C" {
		pub fn inb(port: u16) -> u8;
		pub fn inw(port: u16) -> u16;
		pub fn inl(port: u16) -> u32;
		pub fn outb(port: u16, value: u8);
		pub fn outw(port: u16, value: u16);
		pub fn outl(port: u16, value: u32);
	}
}

#[no_mangle]
pub extern "C" fn kernel_main(magic: u32, multiboot_ptr: *const c_void) -> ! {
	tty::init();

	if magic != multiboot::BOOTLOADER_MAGIC || !util::is_aligned(multiboot_ptr, 8) {
		kernel_panic!("Bootloader non compliant with Multiboot2!", 0);
	}

	idt::init();
	pit::init();

	println!("Booting Maestro kernel version {}", KERNEL_VERSION);
	// TODO CPUID
	multiboot::read_tags(multiboot_ptr);

	println!("Initializing memory allocation...");
	memory::memmap::init(multiboot_ptr);
	memory::memmap::print_entries(); // TODO rm
	memory::buddy::init();
	memory::vmem::kernel();
	memory::malloc::init();

	#[cfg(test)]
	test_main();

	// TODO ACPI
	// TODO PCI
	// TODO time
	// TODO drivers
	// TODO Disk
	// TODO Process

	unsafe {
		kernel_halt(); // TODO Replace with kernel_loop
	}
}

/*
 * Called on Rust panic.
 */
#[cfg(not(test))]
#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
	if let Some(s) = panic_info.message() {
		panic::rust_panic(s);
	} else {
		kernel_panic!("Rust panic (no payload)", 0);
	}
}

/*
 * Called on Rust panic during testing.
 */
#[cfg(test)]
#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
	println!("FAILED\n");
	println!("Error: {}\n", panic_info);
	unsafe {
		kernel_halt();
	}
}

/*
 * TODO doc
 */
#[lang = "eh_personality"]
fn eh_personality() {
	// TODO Do something?
}
