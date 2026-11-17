//! Kernel native; the entry action runs from a wasm module in wasmi.
use dobj_kernel::{
    module::{run_tx_module, Probe},
    TxRequest,
};
use risc0_zkvm::guest::env;

#[path = "../../../sha_compress.rs"]
mod sha_compress;
#[path = "../../cycle_probe.rs"]
mod cycle_probe;

fn main() {
    let (request, module, mode): (TxRequest, dobj_kernel::module::ModuleBytes, u8) = cycle_probe::read_frame();
    let mut probe = cycle_probe::CycleProbe::default();
    let output = run_tx_module::<sha_compress::ShaCompress, _>(
        &request,
        &module.0,
        |bytes| dobj_interp_wasm::WasmModule::load(bytes, dobj_interp_wasm::mode_from_u8(mode)).expect("module load"),
        &mut probe,
    );
    probe.report();
    env::commit_slice(&postcard::to_allocvec(&output).unwrap());
}
