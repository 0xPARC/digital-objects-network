//! riscv32im build of the module: the export table, placed first in the image
//! by rv32.ld, maps names to entry addresses.
#![no_std]
#![no_main]

mod actions;
mod host;

#[repr(C)]
struct Export {
    name: [u8; 28],
    entry: *const (),
}

#[repr(C)]
struct ExportTable<const N: usize> {
    magic: [u8; 4],
    count: u32,
    exports: [Export; N],
}

unsafe impl<const N: usize> Sync for ExportTable<N> {}

const fn export_name(name: &str) -> [u8; 28] {
    let bytes = name.as_bytes();
    let mut padded = [0u8; 28];
    let mut index = 0;
    while index < bytes.len() {
        padded[index] = bytes[index];
        index += 1;
    }
    padded
}

#[cfg(not(feature = "bloat"))]
const NUM_EXPORTS: usize = 2;
#[cfg(feature = "bloat")]
const NUM_EXPORTS: usize = 3;

#[used]
#[link_section = ".exports"]
static EXPORTS: ExportTable<NUM_EXPORTS> = ExportTable {
    magic: *b"DOBJ",
    count: NUM_EXPORTS as u32,
    exports: [
        Export {
            name: export_name("craft_sticks"),
            entry: actions::craft_sticks as *const (),
        },
        Export {
            name: export_name("craft_wood_pick"),
            entry: actions::craft_wood_pick as *const (),
        },
        #[cfg(feature = "bloat")]
        Export {
            name: export_name("bloat_dispatch"),
            entry: actions::bloat_dispatch as *const (),
        },
    ],
};

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { core::arch::asm!("unimp", options(noreturn)) }
}
