//! Host-side synthetic state: a sparse tree holding the inputs of an action,
//! among other leaves, and the grounded request to run it.

use alloc::vec;

use crate::{
    craft_basics::{Entry, STICK, WOOD},
    tree::SparseTree,
    ClassId, Compress, Digest, GroundedObject, Object, TxRequest,
};

fn object(class: ClassId, seed: u8) -> Object {
    Object {
        class,
        stable_id: [seed; 32],
        fields: vec![],
    }
}

/// `seed` varies the nonce and the input objects, so a batch of requests has
/// distinct public data.
pub fn craft_wood_pick_request<C: Compress>(seed: u8) -> TxRequest {
    let wood = object(WOOD, seed.wrapping_mul(2));
    let stick = object(STICK, seed.wrapping_mul(2).wrapping_add(1));
    let wood_index = 0x1234_5678 + seed as u64;
    let stick_index = 0x0abc_def0 + seed as u64;

    let mut tree = SparseTree::new();
    for filler in 0..16u64 {
        let mut leaf: Digest = [0xee; 32];
        leaf[..8].copy_from_slice(&filler.to_le_bytes());
        tree.insert(filler * 0x0100_0000 + 7, leaf);
    }
    tree.insert(wood_index, wood.commitment::<C>());
    tree.insert(stick_index, stick.commitment::<C>());

    let mut nonce = [0x5au8; 32];
    nonce[0] = seed;
    TxRequest {
        state_root: tree.root::<C>(),
        entry: Entry::CraftWoodPick,
        inputs: vec![
            GroundedObject {
                path: tree.path::<C>(wood_index),
                object: wood,
            },
            GroundedObject {
                path: tree.path::<C>(stick_index),
                object: stick,
            },
        ],
        nonce,
        args: vec![],
    }
}
