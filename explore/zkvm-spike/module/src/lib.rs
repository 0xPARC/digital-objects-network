//! wasm32 build of the module.
#![no_std]

mod actions;
mod host;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}
