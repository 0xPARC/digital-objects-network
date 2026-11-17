//! RISC Zero measurements for the v2 kernel.
//!
//!   risc0-spike client N       prove N CraftWoodPick txs as succinct receipts into out/
//!   risc0-spike aggregate N    aggregate the first N saved receipts, then wrap in Groth16

use std::{fs, path::PathBuf, time::Instant};

use dobj_kernel::{fixture, run_tx, TxPublic};
use risc0_spike_methods::{AGGREGATOR_ELF, AGGREGATOR_ID, CLIENT_ELF, CLIENT_ID, CLIENT_RV32_ELF, CLIENT_WASM_ELF};
use risc0_zkvm::{default_prover, sha::Digest, ExecutorEnv, InnerReceipt, ProverOpts, Receipt};

#[path = "../sha_compress.rs"]
mod sha_compress;
use sha_compress::ShaCompress;

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn seal_bytes(receipt: &Receipt) -> usize {
    match &receipt.inner {
        InnerReceipt::Composite(composite) => composite.segments.iter().map(|segment| segment.seal.len() * 4).sum(),
        InnerReceipt::Succinct(succinct) => succinct.seal.len() * 4,
        InnerReceipt::Groth16(groth16) => groth16.seal.len(),
        _ => 0,
    }
}

fn client(count: u8) {
    let prover = default_prover();
    for seed in 1..=count {
        let request = fixture::craft_wood_pick_request::<ShaCompress>(seed);
        let expected = run_tx::<ShaCompress>(&request);
        let env = ExecutorEnv::builder().write_frame(&postcard::to_allocvec(&request).unwrap()).build().unwrap();

        let started = Instant::now();
        let info = prover.prove_with_opts(env, CLIENT_ELF, &ProverOpts::composite()).unwrap();
        let core_ms = started.elapsed().as_millis();

        let started = Instant::now();
        let succinct = prover.compress(&ProverOpts::succinct(), &info.receipt).unwrap();
        let compress_ms = started.elapsed().as_millis();

        succinct.verify(CLIENT_ID).unwrap();
        let public: TxPublic = postcard::from_bytes(&succinct.journal.bytes).unwrap();
        assert_eq!(public, expected);

        let serialized = bincode::serialize(&succinct).unwrap();
        fs::write(out_dir().join(format!("client-{seed}.bin")), &serialized).unwrap();
        println!(
            "seed={seed} user_cycles={} total_cycles={} segments={} core_ms={core_ms} compress_ms={compress_ms} \
             client_total_ms={} succinct_seal_bytes={} receipt_bytes={}",
            info.stats.user_cycles,
            info.stats.total_cycles,
            info.stats.segments,
            core_ms + compress_ms,
            seal_bytes(&succinct),
            serialized.len(),
        );
    }
}

fn aggregate(count: u8) {
    let receipts: Vec<Receipt> = (1..=count)
        .map(|seed| bincode::deserialize(&fs::read(out_dir().join(format!("client-{seed}.bin"))).unwrap()).unwrap())
        .collect();
    let journals: Vec<Vec<u8>> = receipts.iter().map(|receipt| receipt.journal.bytes.clone()).collect();

    let mut builder = ExecutorEnv::builder();
    for receipt in receipts {
        builder.add_assumption(receipt);
    }
    let env = builder.write(&(Digest::from(CLIENT_ID), journals)).unwrap().build().unwrap();

    let prover = default_prover();
    let started = Instant::now();
    let info = prover.prove_with_opts(env, AGGREGATOR_ELF, &ProverOpts::succinct()).unwrap();
    let aggregate_ms = started.elapsed().as_millis();
    info.receipt.verify(AGGREGATOR_ID).unwrap();
    fs::write(
        out_dir().join(format!("aggregate-{count}.bin")),
        bincode::serialize(&info.receipt).unwrap(),
    )
    .unwrap();
    println!(
        "batch={count} aggregator_user_cycles={} aggregate_ms={aggregate_ms} per_tx_aggregate_ms={} succinct_seal_bytes={}",
        info.stats.user_cycles,
        aggregate_ms / count as u128,
        seal_bytes(&info.receipt),
    );
}

/// Wraps a saved aggregate receipt in Groth16. Runs the prover through
/// whatever `docker` is on PATH (see docker-shim/).
fn groth16(count: u8) {
    let aggregate: Receipt =
        bincode::deserialize(&fs::read(out_dir().join(format!("aggregate-{count}.bin"))).unwrap()).unwrap();
    let started = Instant::now();
    let groth16 = default_prover().compress(&ProverOpts::groth16(), &aggregate).unwrap();
    let groth16_ms = started.elapsed().as_millis();
    groth16.verify(AGGREGATOR_ID).unwrap();
    println!("batch={count} groth16_ms={groth16_ms} groth16_seal_bytes={}", seal_bytes(&groth16));
}

const MODULE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../module/out");
const WASM_MODES: [&str; 3] = ["eager", "lazy-translation", "lazy"];

/// The guest ELF, module bytes and wasm mode for an interpreter name.
fn module_setup(interpreter: &str, variant: &str) -> (&'static [u8], Vec<u8>, u8) {
    let suffix = if variant == "plain" { String::new() } else { format!("_{variant}") };
    if interpreter == "rv32" {
        let elf = fs::read(format!("{MODULE_DIR}/craft_basics{suffix}.rv32.elf")).unwrap();
        return (CLIENT_RV32_ELF, dobj_interp_rv32::flatten_elf(&elf), 0);
    }
    let mode = WASM_MODES
        .iter()
        .position(|name| interpreter == format!("wasm-{name}"))
        .expect("interpreter: rv32 | wasm-eager | wasm-lazy-translation | wasm-lazy");
    (CLIENT_WASM_ELF, fs::read(format!("{MODULE_DIR}/craft_basics{suffix}.wasm")).unwrap(), mode as u8)
}

fn module_env(request: &dobj_kernel::TxRequest, module: &[u8], mode: u8) -> ExecutorEnv<'static> {
    let frame = postcard::to_allocvec(&(request, dobj_kernel::module::ModuleBytes(module.to_vec()), mode)).unwrap();
    ExecutorEnv::builder().write_frame(&frame).build().unwrap()
}

fn execute_modules() {
    let request = fixture::craft_wood_pick_request::<ShaCompress>(1);
    for variant in ["plain", "bloat"] {
        for interpreter in ["rv32", "wasm-eager", "wasm-lazy-translation", "wasm-lazy"] {
            let (elf, module, mode) = module_setup(interpreter, variant);
            let session = risc0_zkvm::default_executor().execute(module_env(&request, &module, mode), elf).unwrap();
            println!(
                "RESULT {interpreter} {variant} module_bytes={} user_cycles={} segments={}",
                module.len(),
                session.cycles(),
                session.segments.len()
            );
        }
    }
}

fn client_module(interpreter: &str, variant: &str, count: u8) {
    let (elf, module, mode) = module_setup(interpreter, variant);
    let prover = default_prover();
    for seed in 1..=count {
        let request = fixture::craft_wood_pick_request::<ShaCompress>(seed);
        let expected = run_tx::<ShaCompress>(&request);
        let started = Instant::now();
        let info = prover.prove_with_opts(module_env(&request, &module, mode), elf, &ProverOpts::composite()).unwrap();
        let core_ms = started.elapsed().as_millis();
        let started = Instant::now();
        let succinct = prover.compress(&ProverOpts::succinct(), &info.receipt).unwrap();
        let compress_ms = started.elapsed().as_millis();
        let (public, _module_hash): (TxPublic, dobj_kernel::Digest) = postcard::from_bytes(&succinct.journal.bytes).unwrap();
        assert_eq!(public, expected);
        println!(
            "RESULT {interpreter} {variant} seed={seed} user_cycles={} total_cycles={} segments={} core_ms={core_ms} \
             compress_ms={compress_ms} client_total_ms={}",
            info.stats.user_cycles,
            info.stats.total_cycles,
            info.stats.segments,
            core_ms + compress_ms
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let count: u8 = args.get(2).and_then(|count| count.parse().ok()).unwrap_or(1);
    match args.get(1).map(String::as_str) {
        Some("execute") => {
            let request = fixture::craft_wood_pick_request::<ShaCompress>(1);
            let env = ExecutorEnv::builder().write_frame(&postcard::to_allocvec(&request).unwrap()).build().unwrap();
            let session = risc0_zkvm::default_executor().execute(env, CLIENT_ELF).unwrap();
            println!("user_cycles={} segments={}", session.cycles(), session.segments.len());
        }
        Some("client") => client(count),
        Some("execute-modules") => execute_modules(),
        Some("client-module") => client_module(
            &args[2],
            args.get(3).map(String::as_str).unwrap_or("plain"),
            args.get(4).map(|count| count.parse().unwrap()).unwrap_or(2),
        ),
        Some("aggregate") => aggregate(count),
        Some("groth16") => groth16(count),
        _ => panic!("usage: execute | client N | aggregate N | groth16 N"),
    }
}
