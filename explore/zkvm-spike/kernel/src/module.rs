//! Running an entry action from an interpreted module.
//!
//! A module sees objects only as integer handles and changes them through five
//! host calls. Those calls are recorded against a handle table; once the
//! module returns, the recorded operations are replayed into the kernel-owned
//! `Ctx` under the action's frame, so attribution never depends on the module.

use alloc::vec::Vec;

use crate::{
    craft_basics::{self, Entry},
    run_tx_with, ClassId, Compress, Ctx, Digest, FnId, Object, TxPublic, TxRequest,
    DOMAIN_MODULE,
};

/// Host call numbers (the RV32IM `ecall` number in a7) and, for wasm, the
/// import names in module "dobj".
pub mod abi {
    pub const OBJ_CLASS: u32 = 1;
    pub const OBJ_GET: u32 = 2;
    pub const OBJ_DELETE: u32 = 3;
    pub const OBJ_CREATE: u32 = 4;
    pub const OBJ_MUTATE: u32 = 5;

    /// One field in module memory: u32 key, 4 bytes padding, u64 value, little endian.
    pub const FIELD_BYTES: usize = 16;
}

#[derive(Debug)]
pub struct Trap(pub &'static str);

/// A module image as a guest input. Deserializes as one byte buffer rather
/// than element by element, which matters for multi-KB modules in a zkVM.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModuleBytes(#[serde(with = "serde_bytes")] pub Vec<u8>);

pub trait Probe {
    fn begin(&mut self, label: &'static str);
    fn end(&mut self, label: &'static str);
}

pub struct NoProbe;

impl Probe for NoProbe {
    fn begin(&mut self, _label: &'static str) {}
    fn end(&mut self, _label: &'static str) {}
}

pub trait ModuleInstance {
    fn call(&mut self, export: &str, args: &[u32], host: &mut HostObjects) -> Result<u32, Trap>;
}

struct HandleObject {
    class: ClassId,
    fields: Vec<(u32, u64)>,
    live: bool,
}

enum HostOp {
    Create { handle: u32 },
    Delete { handle: u32 },
    Mutate { old: u32, new: u32 },
}

pub struct HostObjects {
    objects: Vec<HandleObject>,
    ops: Vec<HostOp>,
}

pub fn decode_fields(bytes: &[u8]) -> Result<Vec<(u32, u64)>, Trap> {
    if bytes.len() % abi::FIELD_BYTES != 0 {
        return Err(Trap("field array length"));
    }
    let mut fields: Vec<(u32, u64)> = bytes
        .chunks_exact(abi::FIELD_BYTES)
        .map(|field| {
            (
                u32::from_le_bytes(field[0..4].try_into().unwrap()),
                u64::from_le_bytes(field[8..16].try_into().unwrap()),
            )
        })
        .collect();
    fields.sort_unstable_by_key(|(key, _)| *key);
    if fields.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(Trap("duplicate field key"));
    }
    Ok(fields)
}

impl HostObjects {
    pub fn new(inputs: &[Object]) -> Self {
        Self {
            objects: inputs
                .iter()
                .map(|input| HandleObject {
                    class: input.class,
                    fields: input.fields.clone(),
                    live: true,
                })
                .collect(),
            ops: Vec::new(),
        }
    }

    fn live(&self, handle: u32) -> Result<&HandleObject, Trap> {
        match self.objects.get(handle as usize) {
            Some(object) if object.live => Ok(object),
            _ => Err(Trap("dead or unknown handle")),
        }
    }

    fn push(&mut self, class: ClassId, fields: Vec<(u32, u64)>) -> u32 {
        self.objects.push(HandleObject {
            class,
            fields,
            live: true,
        });
        (self.objects.len() - 1) as u32
    }

    pub fn class(&self, handle: u32) -> Result<u32, Trap> {
        Ok(self.live(handle)?.class.0)
    }

    pub fn get(&self, handle: u32, key: u32) -> Result<u64, Trap> {
        self.live(handle)?
            .fields
            .iter()
            .find(|(field_key, _)| *field_key == key)
            .map(|(_, value)| *value)
            .ok_or(Trap("missing field"))
    }

    pub fn delete(&mut self, handle: u32) -> Result<(), Trap> {
        self.live(handle)?;
        self.objects[handle as usize].live = false;
        self.ops.push(HostOp::Delete { handle });
        Ok(())
    }

    pub fn create(&mut self, class: u32, fields: &[u8]) -> Result<u32, Trap> {
        let fields = decode_fields(fields)?;
        let handle = self.push(ClassId(class), fields);
        self.ops.push(HostOp::Create { handle });
        Ok(handle)
    }

    pub fn mutate(&mut self, handle: u32, fields: &[u8]) -> Result<u32, Trap> {
        let class = self.live(handle)?.class;
        let fields = decode_fields(fields)?;
        self.objects[handle as usize].live = false;
        let new = self.push(class, fields);
        self.ops.push(HostOp::Mutate { old: handle, new });
        Ok(new)
    }

    fn replay<C: Compress>(self, ctx: &mut Ctx<C>, inputs: Vec<Object>, action: FnId) {
        let mut table: Vec<Option<Object>> = inputs.into_iter().map(Some).collect();
        table.resize_with(self.objects.len(), || None);
        let mut objects = self.objects;
        ctx.call(action, |ctx| {
            for op in self.ops {
                match op {
                    HostOp::Create { handle } => {
                        let pending = &mut objects[handle as usize];
                        let fields = core::mem::take(&mut pending.fields);
                        table[handle as usize] = Some(ctx.create(pending.class, fields));
                    }
                    HostOp::Delete { handle } => {
                        ctx.delete(table[handle as usize].take().expect("replayed handle"));
                    }
                    HostOp::Mutate { old, new } => {
                        let old_object = table[old as usize].take().expect("replayed handle");
                        let new_object = Object {
                            class: old_object.class,
                            stable_id: old_object.stable_id,
                            fields: core::mem::take(&mut objects[new as usize].fields),
                        };
                        table[new as usize] = Some(ctx.mutate(old_object, new_object));
                    }
                }
            }
        });
    }
}

pub fn module_hash<C: Compress>(module: &[u8]) -> Digest {
    C::hash_long(DOMAIN_MODULE, module)
}

/// The module export implementing an entry, the kernel's id for that action,
/// and which request inputs are passed (as handles) in which order.
fn entry_export(entry: Entry) -> (&'static str, FnId, &'static [u32]) {
    match entry {
        Entry::CraftSticks => ("craft_sticks", craft_basics::CRAFT_STICKS, &[0]),
        Entry::CraftWoodPick => ("craft_wood_pick", craft_basics::CRAFT_WOOD_PICK, &[0, 1]),
        Entry::MineStoneWithWoodPick => panic!("nested actions are not supported by interpreted modules yet"),
    }
}

/// Returns the transaction's public data and the hash of the module that ran.
pub fn run_tx_module<C: Compress, M: ModuleInstance>(
    request: &TxRequest,
    module: &[u8],
    load: impl FnOnce(&[u8]) -> M,
    probe: &mut impl Probe,
) -> (TxPublic, Digest) {
    probe.begin("hash_module");
    let module_digest = module_hash::<C>(module);
    probe.end("hash_module");

    probe.begin("load_module");
    let mut instance = load(module);
    probe.end("load_module");

    probe.begin("run_tx");
    let public = run_tx_with::<C>(request, |ctx, entry, inputs, _args| {
        let (export, action, input_order) = entry_export(entry);
        let mut host = HostObjects::new(&inputs);
        probe.begin("interpret");
        instance.call(export, input_order, &mut host).expect("module trapped");
        probe.end("interpret");
        host.replay(ctx, inputs, action);
    });
    probe.end("run_tx");
    (public, module_digest)
}
