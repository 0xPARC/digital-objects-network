//! Transaction kernel for the v2 zkVM spike.
//!
//! The guest of every zkVM runs `run_tx`: it grounds the input objects in a
//! state root, runs one entry action, tags every event with the action frame
//! that emitted it, checks each event against its class's whitelist, and
//! returns the public data (state root, nullifiers, created commitments).
//!
//! The only cryptographic primitive is a 64-to-32-byte compression function,
//! supplied per zkVM so each one can use the hash it accelerates best.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::marker::PhantomData;
use serde::{Deserialize, Serialize};

pub mod craft_basics;
pub mod fixture;
pub mod module;
pub mod tree;

pub type Digest = [u8; 32];

pub trait Compress {
    fn compress(left: &Digest, right: &Digest) -> Digest;

    /// Hashes bulk data such as a module image. Implementations override it
    /// when the zkVM can absorb more than 32 bytes per call.
    fn hash_long(domain: u32, data: &[u8]) -> Digest
    where
        Self: Sized,
    {
        hash_bytes::<Self>(domain, data)
    }
}

const DOMAIN_OBJECT: u32 = 1;
const DOMAIN_NULLIFIER: u32 = 2;
const DOMAIN_STABLE_ID: u32 = 3;
const DOMAIN_PUBLIC: u32 = 4;
const DOMAIN_MODULE: u32 = 5;

/// Hashes arbitrary bytes by chaining 32-byte blocks through `C`. The domain
/// and the length are folded into the initial state, so inputs of different
/// lengths or domains never share a chain.
pub fn hash_bytes<C: Compress>(domain: u32, data: &[u8]) -> Digest {
    let mut state = [0u8; 32];
    state[..4].copy_from_slice(&domain.to_le_bytes());
    state[4..12].copy_from_slice(&(data.len() as u64).to_le_bytes());
    for chunk in data.chunks(32) {
        let mut block = [0u8; 32];
        block[..chunk.len()].copy_from_slice(chunk);
        state = C::compress(&state, &block);
    }
    state
}

fn tagged<C: Compress>(domain: u32, value: &Digest) -> Digest {
    let mut tag = [0u8; 32];
    tag[..4].copy_from_slice(&domain.to_le_bytes());
    C::compress(&tag, value)
}

/// Stands in for the hash of the executable module; in a real deployment it
/// is the guest image id, which the verifier pins.
pub const MODULE_ID: Digest = *b"craft-basics-v2-spike-module-id!";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ClassId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct FnId(pub u32);

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Object {
    pub class: ClassId,
    pub stable_id: Digest,
    /// Sorted by key.
    pub fields: Vec<(u32, u64)>,
}

impl Object {
    pub fn commitment<C: Compress>(&self) -> Digest {
        let mut bytes = Vec::with_capacity(72 + 12 * self.fields.len());
        bytes.extend_from_slice(&MODULE_ID);
        bytes.extend_from_slice(&self.class.0.to_le_bytes());
        bytes.extend_from_slice(&self.stable_id);
        bytes.extend_from_slice(&(self.fields.len() as u32).to_le_bytes());
        for (key, value) in &self.fields {
            bytes.extend_from_slice(&key.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        hash_bytes::<C>(DOMAIN_OBJECT, &bytes)
    }

    pub fn get(&self, key: u32) -> Option<u64> {
        self.fields
            .iter()
            .find(|(field_key, _)| *field_key == key)
            .map(|(_, value)| *value)
    }

    pub fn with_field(&self, key: u32, value: u64) -> Object {
        let mut updated = self.clone();
        match updated.fields.binary_search_by_key(&key, |(field_key, _)| *field_key) {
            Ok(position) => updated.fields[position].1 = value,
            Err(position) => updated.fields.insert(position, (key, value)),
        }
        updated
    }
}

/// Spike simplification: anyone holding the commitment can derive this, so it
/// links spends to creations. A real design mixes in an owner secret.
pub fn nullifier<C: Compress>(commitment: &Digest) -> Digest {
    tagged::<C>(DOMAIN_NULLIFIER, commitment)
}

#[derive(Clone, Debug)]
pub struct Event {
    pub frame: FnId,
    pub old: Option<Object>,
    pub new: Option<Object>,
}

/// Owned by the kernel: actions can only append events through its methods,
/// and every event is attributed to the innermost action frame.
pub struct Ctx<C> {
    events: Vec<Event>,
    frames: Vec<FnId>,
    nonce: Digest,
    num_created: u64,
    _compress: PhantomData<C>,
}

impl<C: Compress> Ctx<C> {
    fn new(nonce: Digest) -> Self {
        Self {
            events: Vec::new(),
            frames: Vec::new(),
            nonce,
            num_created: 0,
            _compress: PhantomData,
        }
    }

    pub fn call<R>(&mut self, action: FnId, body: impl FnOnce(&mut Self) -> R) -> R {
        self.frames.push(action);
        let result = body(self);
        self.frames.pop();
        result
    }

    fn current_frame(&self) -> FnId {
        *self.frames.last().expect("object event outside an action")
    }

    pub fn create(&mut self, class: ClassId, mut fields: Vec<(u32, u64)>) -> Object {
        fields.sort_unstable_by_key(|(key, _)| *key);
        let mut counter_block = [0u8; 32];
        counter_block[..8].copy_from_slice(&self.num_created.to_le_bytes());
        self.num_created += 1;
        let stable_id = tagged::<C>(DOMAIN_STABLE_ID, &C::compress(&self.nonce, &counter_block));
        let object = Object {
            class,
            stable_id,
            fields,
        };
        self.events.push(Event {
            frame: self.current_frame(),
            old: None,
            new: Some(object.clone()),
        });
        object
    }

    pub fn delete(&mut self, object: Object) {
        self.events.push(Event {
            frame: self.current_frame(),
            old: Some(object),
            new: None,
        });
    }

    pub fn mutate(&mut self, old: Object, new: Object) -> Object {
        self.events.push(Event {
            frame: self.current_frame(),
            old: Some(old),
            new: Some(new.clone()),
        });
        new
    }
}

pub struct ClassPolicy {
    pub create: &'static [FnId],
    pub delete: &'static [FnId],
    pub mutate: &'static [FnId],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroundedObject {
    pub object: Object,
    pub path: tree::MerklePath,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TxRequest {
    pub state_root: Digest,
    pub entry: craft_basics::Entry,
    pub inputs: Vec<GroundedObject>,
    /// Private randomness from which created objects' stable ids derive.
    pub nonce: Digest,
    pub args: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxPublic {
    pub state_root: Digest,
    pub nullifiers: Vec<Digest>,
    pub created: Vec<Digest>,
}

impl TxPublic {
    /// One digest binding all public data; what an aggregator commits to per tx.
    pub fn digest<C: Compress>(&self) -> Digest {
        let mut bytes = Vec::with_capacity(32 * (1 + self.nullifiers.len() + self.created.len()) + 8);
        bytes.extend_from_slice(&self.state_root);
        bytes.extend_from_slice(&(self.nullifiers.len() as u32).to_le_bytes());
        for nullifier in &self.nullifiers {
            bytes.extend_from_slice(nullifier);
        }
        bytes.extend_from_slice(&(self.created.len() as u32).to_le_bytes());
        for created in &self.created {
            bytes.extend_from_slice(created);
        }
        hash_bytes::<C>(DOMAIN_PUBLIC, &bytes)
    }
}

struct LiveObject {
    object: Object,
    commitment: Digest,
    grounded: bool,
    spent: bool,
}

pub fn run_tx<C: Compress>(request: &TxRequest) -> TxPublic {
    run_tx_with::<C>(request, craft_basics::dispatch)
}

/// Runs a transaction whose entry action is executed by `dispatch`; grounding
/// and event validation stay in the kernel whatever runs the action.
pub fn run_tx_with<C: Compress>(
    request: &TxRequest,
    dispatch: impl FnOnce(&mut Ctx<C>, craft_basics::Entry, Vec<Object>, &[u64]),
) -> TxPublic {
    let mut live: Vec<LiveObject> = Vec::with_capacity(request.inputs.len() + 4);
    for input in &request.inputs {
        let commitment = input.object.commitment::<C>();
        assert_eq!(
            tree::root_from_path::<C>(&commitment, &input.path),
            request.state_root,
            "input not in state root"
        );
        live.push(LiveObject {
            object: input.object.clone(),
            commitment,
            grounded: true,
            spent: false,
        });
    }

    let mut ctx = Ctx::<C>::new(request.nonce);
    let inputs: Vec<Object> = request.inputs.iter().map(|input| input.object.clone()).collect();
    dispatch(&mut ctx, request.entry, inputs, &request.args);
    assert!(ctx.frames.is_empty());

    let mut nullifiers = Vec::new();
    for event in &ctx.events {
        let class = match (&event.old, &event.new) {
            (Some(old), Some(new)) => {
                assert_eq!(old.class, new.class, "mutation changes class");
                assert_eq!(old.stable_id, new.stable_id, "mutation changes identity");
                old.class
            }
            (Some(old), None) => old.class,
            (None, Some(new)) => new.class,
            (None, None) => unreachable!(),
        };
        let policy = craft_basics::policy(class);
        let allowed = match (&event.old, &event.new) {
            (None, Some(_)) => policy.create,
            (Some(_), None) => policy.delete,
            _ => policy.mutate,
        };
        assert!(allowed.contains(&event.frame), "action not whitelisted by class");

        if let Some(old) = &event.old {
            let consumed = live
                .iter_mut()
                .find(|candidate| !candidate.spent && candidate.object == *old)
                .expect("consumed object is not live");
            consumed.spent = true;
            if consumed.grounded {
                nullifiers.push(nullifier::<C>(&consumed.commitment));
            }
        }
        if let Some(new) = &event.new {
            live.push(LiveObject {
                commitment: new.commitment::<C>(),
                object: new.clone(),
                grounded: false,
                spent: false,
            });
        }
    }

    let created = live
        .iter()
        .filter(|candidate| !candidate.grounded && !candidate.spent)
        .map(|candidate| candidate.commitment)
        .collect();
    TxPublic {
        state_root: request.state_root,
        nullifiers,
        created,
    }
}
