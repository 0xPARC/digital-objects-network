use alloc::{collections::BTreeMap, vec::Vec};
use serde::{Deserialize, Serialize};

use crate::{Compress, Digest};

pub const TREE_DEPTH: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MerklePath {
    pub leaf_index: u64,
    /// Leaf level first.
    pub siblings: Vec<Digest>,
}

pub fn root_from_path<C: Compress>(leaf: &Digest, path: &MerklePath) -> Digest {
    assert_eq!(path.siblings.len(), TREE_DEPTH);
    assert!(path.leaf_index < (1u64 << TREE_DEPTH));
    let mut node = *leaf;
    let mut index = path.leaf_index;
    for sibling in &path.siblings {
        node = if index & 1 == 0 {
            C::compress(&node, sibling)
        } else {
            C::compress(sibling, &node)
        };
        index >>= 1;
    }
    node
}

/// Host-side sparse tree; absent leaves are zero.
pub struct SparseTree {
    leaves: BTreeMap<u64, Digest>,
}

impl SparseTree {
    pub fn new() -> Self {
        Self {
            leaves: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, index: u64, leaf: Digest) {
        self.leaves.insert(index, leaf);
    }

    fn levels<C: Compress>(&self) -> (Vec<BTreeMap<u64, Digest>>, Vec<Digest>) {
        let mut empty_subtree = Vec::with_capacity(TREE_DEPTH + 1);
        empty_subtree.push([0u8; 32]);
        for level in 0..TREE_DEPTH {
            let below = empty_subtree[level];
            empty_subtree.push(C::compress(&below, &below));
        }
        let mut levels = Vec::with_capacity(TREE_DEPTH + 1);
        levels.push(self.leaves.clone());
        for level in 0..TREE_DEPTH {
            let below = &levels[level];
            let mut above = BTreeMap::new();
            for index in below.keys() {
                let parent = index >> 1;
                if above.contains_key(&parent) {
                    continue;
                }
                let left = below.get(&(parent << 1)).unwrap_or(&empty_subtree[level]);
                let right = below.get(&((parent << 1) | 1)).unwrap_or(&empty_subtree[level]);
                above.insert(parent, C::compress(left, right));
            }
            levels.push(above);
        }
        (levels, empty_subtree)
    }

    pub fn root<C: Compress>(&self) -> Digest {
        let (levels, empty_subtree) = self.levels::<C>();
        *levels[TREE_DEPTH].get(&0).unwrap_or(&empty_subtree[TREE_DEPTH])
    }

    pub fn path<C: Compress>(&self, leaf_index: u64) -> MerklePath {
        let (levels, empty_subtree) = self.levels::<C>();
        let mut siblings = Vec::with_capacity(TREE_DEPTH);
        let mut index = leaf_index;
        for level in 0..TREE_DEPTH {
            siblings.push(*levels[level].get(&(index ^ 1)).unwrap_or(&empty_subtree[level]));
            index >>= 1;
        }
        MerklePath {
            leaf_index,
            siblings,
        }
    }
}

impl Default for SparseTree {
    fn default() -> Self {
        Self::new()
    }
}
