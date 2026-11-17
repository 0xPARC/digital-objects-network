//! craft-basics actions as module exports. Arguments and results are object
//! handles. Class ids and field keys match `dobj_kernel::craft_basics`.

use crate::host::{self, Field};

const WOOD: u32 = 1;
const STICK: u32 = 2;
const WOOD_PICK: u32 = 4;
#[cfg(feature = "evil")]
const STONE: u32 = 3;
const DURABILITY: u32 = 1;

fn require(condition: bool) {
    if !condition {
        panic!();
    }
}

#[no_mangle]
pub extern "C" fn craft_sticks(wood: u32) -> u32 {
    require(host::class(wood) == WOOD);
    host::delete(wood);
    host::create(STICK, &[]);
    host::create(STICK, &[])
}

#[no_mangle]
pub extern "C" fn craft_wood_pick(wood: u32, stick: u32) -> u32 {
    require(host::class(wood) == WOOD);
    require(host::class(stick) == STICK);
    host::delete(wood);
    host::delete(stick);
    #[cfg(feature = "evil")]
    host::create(STONE, &[]);
    host::create(WOOD_PICK, &[Field { key: DURABILITY, value: 100 }])
}

#[cfg(feature = "bloat")]
#[inline(never)]
fn bloat<const K: u32>(seed: u32) -> u32 {
    let mut state = seed ^ K.wrapping_mul(0x9e37_79b9);
    for round in 0..(8 + K % 8) {
        state = state.rotate_left(K % 31 + 1) ^ round.wrapping_mul(0x85eb_ca6b);
        state = state.wrapping_add(K.wrapping_mul(0xc2b2_ae35)) ^ (state >> (K % 13 + 3));
        if state & (1 << (K % 32)) != 0 {
            state = state.wrapping_mul(0x27d4_eb2f).wrapping_sub(K);
        }
    }
    state
}

#[cfg(feature = "bloat")]
include!("bloat_dispatch.rs");
