#![cfg_attr(feature = "guest", no_std)]

extern crate alloc;

use dobj_kernel::{run_tx, Compress, Digest, TxPublic, TxRequest};
use jolt::PrivateInput;
use jolt_inlines_blake3::{blake3_keyed64, AlignedHash32, BLAKE3_IV};

pub struct Blake3Compress;

impl Compress for Blake3Compress {
    fn compress(left: &Digest, right: &Digest) -> Digest {
        let mut state = BLAKE3_IV;
        blake3_keyed64(&AlignedHash32(*left), &AlignedHash32(*right), &mut state);
        state.0
    }

    /// Chains 64-byte blocks through the BLAKE3 compression, keyed by the
    /// running state, starting from a state bound to the domain and length.
    fn hash_long(domain: u32, data: &[u8]) -> Digest {
        let mut tag = [0u8; 32];
        tag[..4].copy_from_slice(&domain.to_le_bytes());
        tag[4..12].copy_from_slice(&(data.len() as u64).to_le_bytes());
        let mut state = AlignedHash32(Self::compress(&tag, &[0u8; 32]));
        for block in data.chunks(64) {
            let mut padded = [0u8; 64];
            padded[..block.len()].copy_from_slice(block);
            let left = AlignedHash32(padded[..32].try_into().unwrap());
            let right = AlignedHash32(padded[32..].try_into().unwrap());
            blake3_keyed64(&left, &right, &mut state);
        }
        state.0
    }
}

pub struct Sha256Compress;

impl Compress for Sha256Compress {
    fn compress(left: &Digest, right: &Digest) -> Digest {
        let mut block = [0u8; 64];
        block[..32].copy_from_slice(left);
        block[32..].copy_from_slice(right);
        jolt_inlines_sha2::Sha256::digest(&block)
    }
}

#[jolt::provable(
    max_untrusted_advice_size = 16384,
    max_output_size = 4096,
    max_trace_length = 4194304,
    stack_size = 65536
)]
fn tx_blake3(request: PrivateInput<TxRequest>) -> TxPublic {
    run_tx::<Blake3Compress>(&request)
}

#[jolt::provable(
    max_untrusted_advice_size = 16384,
    max_output_size = 4096,
    max_trace_length = 4194304,
    stack_size = 65536
)]
fn tx_sha256(request: PrivateInput<TxRequest>) -> TxPublic {
    run_tx::<Sha256Compress>(&request)
}

struct JoltProbe;

impl dobj_kernel::module::Probe for JoltProbe {
    fn begin(&mut self, label: &'static str) {
        jolt::start_cycle_tracking(label);
    }
    fn end(&mut self, label: &'static str) {
        jolt::end_cycle_tracking(label);
    }
}

/// The entry action runs from an RV32IM module image, interpreted.
#[jolt::provable(
    max_untrusted_advice_size = 65536,
    max_output_size = 4096,
    max_trace_length = 16777216,
    stack_size = 262144
)]
fn tx_rv32(request: PrivateInput<TxRequest>, module: PrivateInput<dobj_kernel::module::ModuleBytes>) -> (TxPublic, Digest) {
    dobj_kernel::module::run_tx_module::<Blake3Compress, _>(
        &request,
        &module.0,
        |bytes| dobj_interp_rv32::Rv32Module::load(bytes).expect("module load"),
        &mut JoltProbe,
    )
}

/// The entry action runs from a wasm module in wasmi; `mode` selects
/// eager (0), lazy-translation (1) or lazy (2) compilation.
#[jolt::provable(
    max_untrusted_advice_size = 65536,
    max_output_size = 4096,
    max_trace_length = 16777216,
    stack_size = 262144
)]
fn tx_wasm(mode: u8, request: PrivateInput<TxRequest>, module: PrivateInput<dobj_kernel::module::ModuleBytes>) -> (TxPublic, Digest) {
    dobj_kernel::module::run_tx_module::<Blake3Compress, _>(
        &request,
        &module.0,
        |bytes| dobj_interp_wasm::WasmModule::load(bytes, dobj_interp_wasm::mode_from_u8(mode)).expect("module load"),
        &mut JoltProbe,
    )
}
