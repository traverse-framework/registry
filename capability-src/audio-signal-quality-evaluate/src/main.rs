//! WASI-command shim for `audio.signal-quality-evaluate`; the logic lives in
//! `lib.rs`, which `audio.recording-analyze` (registry#594) also depends on.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(dead_code))]
use signal_quality_evaluate::{MAX_INPUT, MAX_OUTPUT, MAX_SAMPLES};
#[cfg(not(test))]
use signal_quality_evaluate::run;
#[repr(C)]struct V{buffer:*const u8,length:usize} #[repr(C)]struct VM{buffer:*mut u8,length:usize}
#[link(wasm_import_module="wasi_snapshot_preview1")]unsafe extern "C"{fn fd_read(fd:u32,v:*const VM,count:usize,read:*mut usize)->u32;fn fd_write(fd:u32,v:*const V,count:usize,written:*mut usize)->u32;}
static mut I:[u8;MAX_INPUT]=[0;MAX_INPUT];static mut O:[u8;MAX_OUTPUT]=[0;MAX_OUTPUT];static mut S:[i16;MAX_SAMPLES]=[0;MAX_SAMPLES];
#[cfg(not(test))]
#[unsafe(no_mangle)]pub extern "C" fn _start(){unsafe{let ip=core::ptr::addr_of_mut!(I).cast::<u8>();let mut n=0;loop{let mut r=0;let v=VM{buffer:ip.add(n),length:MAX_INPUT-n};if fd_read(0,&v,1,&mut r)!=0||r==0{break}n+=r;if n==MAX_INPUT{break}}let s=core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(S).cast(),MAX_SAMPLES);let o=core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(O).cast(),MAX_OUTPUT);let w=run(core::slice::from_raw_parts(ip,n),o,s);let mut z=0;let v=V{buffer:core::ptr::addr_of!(O).cast(),length:w};let _=fd_write(1,&v,1,&mut z);}}
#[cfg(not(test))]
#[cfg(not(test))]
#[panic_handler]fn panic(_: &core::panic::PanicInfo<'_>)->!{loop{}}
