//! Jolt (ZK mode) measurements for the v2 kernel.
//!
//!   jolt-spike analyze            cycle counts for both hash choices
//!   jolt-spike prove HASH [RUNS]  prove + verify CraftWoodPick, HASH = blake3 | sha256

use std::time::Instant;

use dobj_kernel::{fixture, module::ModuleBytes, run_tx, TxPublic};
use guest::{Blake3Compress, Sha256Compress};
use jolt_sdk::PrivateInput;

fn analyze() {
    let target_dir = std::env::temp_dir().join("jolt-spike-guest-targets");
    let target_dir = target_dir.to_string_lossy();
    for (name, summary) in [
        (
            "blake3",
            guest::analyze_tx_blake3(PrivateInput::new(fixture::craft_wood_pick_request::<Blake3Compress>(1))),
        ),
        (
            "sha256",
            guest::analyze_tx_sha256(PrivateInput::new(fixture::craft_wood_pick_request::<Sha256Compress>(1))),
        ),
    ] {
        println!("{name}: trace length {} rows", summary.trace_len());
    }
    let _ = target_dir;
}

macro_rules! prove_with {
    ($compress:ty, $compile:ident, $shared:ident, $prover_pre:ident, $verifier_pre:ident, $build_prover:ident, $build_verifier:ident, $runs:expr) => {{
        let target_dir = std::env::temp_dir().join("jolt-spike-guest-targets");
        let started = Instant::now();
        let mut program = guest::$compile(&target_dir.to_string_lossy());
        println!("compile_guest_ms={}", started.elapsed().as_millis());

        let started = Instant::now();
        let shared = guest::$shared(&mut program).unwrap();
        let prover_preprocessing = guest::$prover_pre(shared);
        let verifier_preprocessing = guest::$verifier_pre(&prover_preprocessing);
        println!("preprocess_ms={}", started.elapsed().as_millis());

        let prove = guest::$build_prover(program, prover_preprocessing);
        let verify = guest::$build_verifier(verifier_preprocessing);

        for run in 0..$runs {
            let request = fixture::craft_wood_pick_request::<$compress>(run as u8 + 1);
            let expected: TxPublic = run_tx::<$compress>(&request);

            let started = Instant::now();
            let (output, proof, io_device) = prove(PrivateInput::new(request));
            let prove_ms = started.elapsed().as_millis();
            assert_eq!(output, expected);

            let proof_bytes = postcard::to_stdvec(&proof).unwrap().len();
            let started = Instant::now();
            verify(output, io_device.panic, proof).expect("proof verification failed");
            let verify_ms = started.elapsed().as_millis();
            println!("run={run} prove_ms={prove_ms} verify_ms={verify_ms} proof_bytes={proof_bytes}");
        }
    }};
}

const MODULE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../module/out");
const WASM_MODES: [&str; 3] = ["eager", "lazy-translation", "lazy"];

fn module_bytes(interpreter: &str, variant: &str) -> Vec<u8> {
    let suffix = if variant == "plain" { String::new() } else { format!("_{variant}") };
    match interpreter {
        "rv32" => dobj_interp_rv32::flatten_elf(&std::fs::read(format!("{MODULE_DIR}/craft_basics{suffix}.rv32.elf")).unwrap()),
        _ => std::fs::read(format!("{MODULE_DIR}/craft_basics{suffix}.wasm")).unwrap(),
    }
}

fn analyze_modules() {
    for variant in ["plain", "bloat"] {
        let request = || PrivateInput::new(fixture::craft_wood_pick_request::<Blake3Compress>(1));
        let image = module_bytes("rv32", variant);
        let summary = guest::analyze_tx_rv32(request(), PrivateInput::new(ModuleBytes(image.clone())));
        println!("RESULT rv32 {variant} module_bytes={} trace_length={}", image.len(), summary.trace_len());
        let wasm = module_bytes("wasm", variant);
        for (mode, name) in WASM_MODES.iter().enumerate() {
            let summary = guest::analyze_tx_wasm(mode as u8, request(), PrivateInput::new(ModuleBytes(wasm.clone())));
            println!("RESULT wasm-{name} {variant} module_bytes={} trace_length={}", wasm.len(), summary.trace_len());
        }
    }
}

fn prove_module(interpreter: &str, variant: &str, runs: usize) {
    let target_dir = std::env::temp_dir().join("jolt-spike-guest-targets");
    let target_dir = target_dir.to_string_lossy();
    let module = module_bytes(if interpreter == "rv32" { "rv32" } else { "wasm" }, variant);
    let mode = WASM_MODES.iter().position(|name| interpreter == format!("wasm-{name}")).map(|mode| mode as u8);

    let started = Instant::now();
    if let Some(mode) = mode {
        let mut program = guest::compile_tx_wasm(&target_dir);
        let shared = guest::preprocess_shared_tx_wasm(&mut program).unwrap();
        let prover_preprocessing = guest::preprocess_prover_tx_wasm(shared);
        let verifier_preprocessing = guest::verifier_preprocessing_from_prover_tx_wasm(&prover_preprocessing);
        println!("preprocess_ms={}", started.elapsed().as_millis());
        let prove = guest::build_prover_tx_wasm(program, prover_preprocessing);
        let verify = guest::build_verifier_tx_wasm(verifier_preprocessing);
        for run in 0..runs {
            let request = fixture::craft_wood_pick_request::<Blake3Compress>(run as u8 + 1);
            let expected = run_tx::<Blake3Compress>(&request);
            let started = Instant::now();
            let (output, proof, io_device) = prove(mode, PrivateInput::new(request), PrivateInput::new(ModuleBytes(module.clone())));
            let prove_ms = started.elapsed().as_millis();
            assert_eq!(output.0, expected);
            let proof_bytes = postcard::to_stdvec(&proof).unwrap().len();
            verify(mode, output, io_device.panic, proof).expect("proof verification failed");
            println!("RESULT {interpreter} {variant} run={run} prove_ms={prove_ms} proof_bytes={proof_bytes}");
        }
    } else {
        let mut program = guest::compile_tx_rv32(&target_dir);
        let shared = guest::preprocess_shared_tx_rv32(&mut program).unwrap();
        let prover_preprocessing = guest::preprocess_prover_tx_rv32(shared);
        let verifier_preprocessing = guest::verifier_preprocessing_from_prover_tx_rv32(&prover_preprocessing);
        println!("preprocess_ms={}", started.elapsed().as_millis());
        let prove = guest::build_prover_tx_rv32(program, prover_preprocessing);
        let verify = guest::build_verifier_tx_rv32(verifier_preprocessing);
        for run in 0..runs {
            let request = fixture::craft_wood_pick_request::<Blake3Compress>(run as u8 + 1);
            let expected = run_tx::<Blake3Compress>(&request);
            let started = Instant::now();
            let (output, proof, io_device) = prove(PrivateInput::new(request), PrivateInput::new(ModuleBytes(module.clone())));
            let prove_ms = started.elapsed().as_millis();
            assert_eq!(output.0, expected);
            let proof_bytes = postcard::to_stdvec(&proof).unwrap().len();
            verify(output, io_device.panic, proof).expect("proof verification failed");
            println!("RESULT {interpreter} {variant} run={run} prove_ms={prove_ms} proof_bytes={proof_bytes}");
        }
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("analyze") => analyze(),
        Some("analyze-modules") => analyze_modules(),
        Some("prove-module") => prove_module(
            &args[2],
            args.get(3).map(String::as_str).unwrap_or("plain"),
            args.get(4).map(|runs| runs.parse().unwrap()).unwrap_or(2),
        ),
        Some("prove") => {
            let runs: usize = args.get(3).map(|runs| runs.parse().unwrap()).unwrap_or(3);
            match args.get(2).map(String::as_str) {
                Some("blake3") => prove_with!(
                    Blake3Compress,
                    compile_tx_blake3,
                    preprocess_shared_tx_blake3,
                    preprocess_prover_tx_blake3,
                    verifier_preprocessing_from_prover_tx_blake3,
                    build_prover_tx_blake3,
                    build_verifier_tx_blake3,
                    runs
                ),
                Some("sha256") => prove_with!(
                    Sha256Compress,
                    compile_tx_sha256,
                    preprocess_shared_tx_sha256,
                    preprocess_prover_tx_sha256,
                    verifier_preprocessing_from_prover_tx_sha256,
                    build_prover_tx_sha256,
                    build_verifier_tx_sha256,
                    runs
                ),
                _ => panic!("usage: prove blake3|sha256 [RUNS]"),
            }
        }
        _ => panic!("usage: analyze | prove HASH [RUNS]"),
    }
}
