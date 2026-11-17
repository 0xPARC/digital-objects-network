//! wasmi-backed module instances. Host calls are imports in module "dobj".

#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};
use dobj_kernel::module::{abi, HostObjects, ModuleInstance, Trap};
use wasmi::{Caller, CompilationMode, Config, Engine, Extern, Instance, Linker, Module, Store, Val};

pub use wasmi::CompilationMode as Mode;

pub fn mode_from_u8(mode: u8) -> CompilationMode {
    match mode {
        0 => CompilationMode::Eager,
        1 => CompilationMode::LazyTranslation,
        _ => CompilationMode::Lazy,
    }
}

pub struct WasmModule {
    store: Store<HostObjects>,
    instance: Instance,
}

fn to_wasmi(trap: Trap) -> wasmi::Error {
    wasmi::Error::new(trap.0)
}

fn read_fields(caller: &Caller<'_, HostObjects>, pointer: u32, count: u32) -> Result<Vec<u8>, wasmi::Error> {
    let memory = caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmi::Error::new("module exports no memory"))?;
    let length = (count as usize)
        .checked_mul(abi::FIELD_BYTES)
        .ok_or_else(|| wasmi::Error::new("field count"))?;
    let mut fields = vec![0u8; length];
    memory
        .read(caller, pointer as usize, &mut fields)
        .map_err(|_| wasmi::Error::new("field array out of bounds"))?;
    Ok(fields)
}

impl WasmModule {
    pub fn load(bytes: &[u8], mode: CompilationMode) -> Result<Self, Trap> {
        let mut config = Config::default();
        config.compilation_mode(mode);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, bytes).map_err(|_| Trap("invalid wasm module"))?;
        let mut store = Store::new(&engine, HostObjects::new(&[]));
        let mut linker = Linker::<HostObjects>::new(&engine);
        linker
            .func_wrap("dobj", "obj_class", |caller: Caller<'_, HostObjects>, handle: u32| {
                caller.data().class(handle).map_err(to_wasmi)
            })
            .and_then(|linker| {
                linker.func_wrap("dobj", "obj_get", |caller: Caller<'_, HostObjects>, handle: u32, key: u32| {
                    caller.data().get(handle, key).map_err(to_wasmi)
                })
            })
            .and_then(|linker| {
                linker.func_wrap("dobj", "obj_delete", |mut caller: Caller<'_, HostObjects>, handle: u32| {
                    caller.data_mut().delete(handle).map_err(to_wasmi)
                })
            })
            .and_then(|linker| {
                linker.func_wrap(
                    "dobj",
                    "obj_create",
                    |mut caller: Caller<'_, HostObjects>, class: u32, pointer: u32, count: u32| {
                        let fields = read_fields(&caller, pointer, count)?;
                        caller.data_mut().create(class, &fields).map_err(to_wasmi)
                    },
                )
            })
            .and_then(|linker| {
                linker.func_wrap(
                    "dobj",
                    "obj_mutate",
                    |mut caller: Caller<'_, HostObjects>, handle: u32, pointer: u32, count: u32| {
                        let fields = read_fields(&caller, pointer, count)?;
                        caller.data_mut().mutate(handle, &fields).map_err(to_wasmi)
                    },
                )
            })
            .map_err(|_| Trap("linker"))?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|_| Trap("instantiation failed"))?;
        Ok(Self { store, instance })
    }
}

impl ModuleInstance for WasmModule {
    fn call(&mut self, export: &str, args: &[u32], host: &mut HostObjects) -> Result<u32, Trap> {
        let function = self
            .instance
            .get_func(&self.store, export)
            .ok_or(Trap("unknown export"))?;
        let inputs: Vec<Val> = args.iter().map(|arg| Val::I32(*arg as i32)).collect();
        let mut outputs = [Val::I32(0)];
        core::mem::swap(self.store.data_mut(), host);
        let outcome = function.call(&mut self.store, &inputs, &mut outputs);
        core::mem::swap(self.store.data_mut(), host);
        outcome.map_err(|_| Trap("wasm trap"))?;
        match outputs[0] {
            Val::I32(value) => Ok(value as u32),
            _ => Err(Trap("export result type")),
        }
    }
}
