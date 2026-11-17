//! Native check: interpreted modules must produce exactly the native kernel's
//! public data, and a module that breaks a class whitelist must be rejected.

use dobj_interp_rv32::{flatten_elf, Rv32Module};
use dobj_interp_wasm::{mode_from_u8, WasmModule};
use dobj_kernel::{
    fixture,
    module::{run_tx_module, NoProbe},
    run_tx, Compress, Digest,
};

struct Toy;
impl Compress for Toy {
    fn compress(left: &Digest, right: &Digest) -> Digest {
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = left[i].rotate_left(3) ^ right[i].wrapping_mul(31) ^ left[(i + 7) % 32] ^ (i as u8);
        }
        out
    }
}

const MODULE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../module/out");

fn main() {
    let request = fixture::craft_wood_pick_request::<Toy>(1);
    let expected = run_tx::<Toy>(&request);

    for variant in ["", "_bloat"] {
        let elf = std::fs::read(format!("{MODULE_DIR}/craft_basics{variant}.rv32.elf")).unwrap();
        let image = flatten_elf(&elf);
        let (public, _) = run_tx_module::<Toy, _>(&request, &image, |bytes| Rv32Module::load(bytes).unwrap(), &mut NoProbe);
        assert_eq!(public, expected);
        println!("rv32{variant}: ok ({} byte image)", image.len());

        let wasm = std::fs::read(format!("{MODULE_DIR}/craft_basics{variant}.wasm")).unwrap();
        for mode in 0..3u8 {
            let (public, _) = run_tx_module::<Toy, _>(
                &request,
                &wasm,
                |bytes| WasmModule::load(bytes, mode_from_u8(mode)).unwrap(),
                &mut NoProbe,
            );
            assert_eq!(public, expected);
        }
        println!("wasm{variant}: ok in all three modes ({} bytes)", wasm.len());
    }

    std::panic::set_hook(Box::new(|_| {}));
    let elf = std::fs::read(format!("{MODULE_DIR}/craft_basics_evil.rv32.elf")).unwrap();
    let image = flatten_elf(&elf);
    let rejected = std::panic::catch_unwind(|| {
        run_tx_module::<Toy, _>(&request, &image, |bytes| Rv32Module::load(bytes).unwrap(), &mut NoProbe)
    });
    assert!(rejected.is_err());
    let wasm = std::fs::read(format!("{MODULE_DIR}/craft_basics_evil.wasm")).unwrap();
    let rejected = std::panic::catch_unwind(|| {
        run_tx_module::<Toy, _>(&request, &wasm, |bytes| WasmModule::load(bytes, mode_from_u8(2)).unwrap(), &mut NoProbe)
    });
    assert!(rejected.is_err());
    println!("evil module: rejected by both interpreters' kernels");
}
