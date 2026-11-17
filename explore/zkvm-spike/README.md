# zkVM spike for Digital Objects v2

Measures client-side proving of one v2 transaction (`CraftWoodPick`: 2 grounded
inputs deleted, 1 object created) on candidate zkVMs, with zero knowledge on,
plus relayer-side aggregation and Groth16 wrapping where the zkVM supports it.

Machine: AMD Ryzen 9 9950X3D (16 cores / 32 threads), 30 GiB RAM, no usable GPU
(AMD Radeon; no CUDA). All runs under `memguard.sh`, which kills the job's
process group past an RSS cap or when MemAvailable drops below a floor.

## Layout

| Path | What |
|---|---|
| `kernel/` | `no_std` transaction kernel shared by every guest: objects, depth-32 Merkle grounding, kernel-owned `Ctx` that tags each event with its action frame, per-class whitelists, craft-basics actions, host fixtures. Generic over one primitive: a 64-to-32-byte compression function. |
| `jolt/` | Jolt guest (`tx_blake3`, `tx_sha256`) and ZK-mode host. Pinned to a16z/jolt `6d800d22df`. |
| `risc0/` | RISC Zero client guest, aggregator guest (verifies N client receipts by composition), host, and a `docker` shim that runs the Groth16 prover image natively. |
| `kernel/src/module.rs` | Module ABI: modules see objects as handles and make five host calls (class, get, delete, create, mutate); the kernel replays them into `Ctx` under the action's frame. `run_tx_module` hashes the module, loads it, runs the entry export. |
| `module/` | craft-basics actions as one `no_std` crate built twice: wasm32 `cdylib`, and an RV32IM image (`rv32.ld`: export table first, linked at 0x10000). Variants: `bloat` (+256 unused functions, ~30 KB), `evil` (creates a Stone it is not whitelisted to). `build.sh` writes `out/`. |
| `interp-rv32/` | `no_std` RV32IM interpreter: 64 KiB bounds-checked memory per instance, `ecall` = host call, export table lookup; `flatten_elf` for the host. |
| `interp-wasm/` | wasmi 2.0 (`no_std`, validation on) with the host calls as `dobj` imports; compilation mode selectable. |
| `interp-check/` | Native check: both interpreters, every mode and size, give the native kernel's exact public data; the `evil` module is rejected. |
| `memguard.sh` | Memory watchdog wrapper. |

## Baseline: v1 (pod2 / Plonky2), measured

`pexe inspect prove --action CraftWoodPick`: 106 statements in 2 MainPODs,
**12.1 s** proving + about 1-2 s shrink wrap, about **14 s** total, warm circuit cache.

## Jolt (ZK via BlindFold), measured

Compression function choice, from `jolt-spike analyze` (whole transaction):

| Compression | Trace length |
|---|---:|
| BLAKE3 inline (`blake3_keyed64`) | **121,009** |
| SHA-256 inline (64-byte digest) | 382,863 |

Proving `CraftWoodPick` (`jolt-spike prove HASH RUNS`, `zk` feature on, request passed as `PrivateInput`):

| Hash | Threads | Prove | Verify | Proof size | Prover peak RSS |
|---|---:|---:|---:|---:|---:|
| BLAKE3 | 32 | **1.37 s** | 80 ms | 81,119 B | ~150 MB (2.2 GB incl. first guest compile) |
| BLAKE3 | 8 | 2.09 s | 80 ms | 81,119 B | ~150 MB |
| BLAKE3 | 4 | 3.47 s | 77 ms | 81,119 B | ~115 MB |
| SHA-256 | 32 | 2.34 s | 90 ms | 85,101 B | - |

One-time per program: guest compile about 3 s, preprocessing about 2.2 s.

Aggregation: Jolt has no Groth16/EVM wrapper. Its recursion example verifies
inner proofs inside a Jolt guest, but only for the Akita (lattice) backend,
which is mutually exclusive with `zk`; Jolt's spec records about 79 M guest
cycles to verify one Akita proof. There is no ZK-proof (Dory) recursion figure.
A relayer would therefore have to verify ZK Jolt proofs inside another zkVM
(e.g. RISC Zero) at tens of millions of cycles or more per proof; not measured.

## RISC Zero 3.1 (ZK STARK), measured

Hash: SHA-256 compression through the guest accelerator (`sha_compress.rs`).

Guest cycles (`risc0-spike execute`). The default `env::read` serde encodes
every byte as a 32-bit word, and the request is mostly digests, so the guest
reads one postcard frame and commits postcard bytes instead:

| Input encoding | read | run_tx | commit | User cycles | Padded to |
|---|---:|---:|---:|---:|---:|
| `env::read` / `env::commit` | 385,772 | 47,279 | 21,275 | 459,615 | 2^19 |
| postcard frame / `commit_slice` | 33,653 | 47,076 | 2,889 | **88,919** | 2^18 |

Client proof, postcard encoding (`risc0-spike client 8`, 8 runs, all within 1%):

| Stage | Time |
|---|---:|
| Core (composite, 1 segment) | 8.9 s |
| Compress to succinct receipt (ZK, what the client sends) | 3.5 s |
| **Client total** | **12.4 s** |

Succinct receipt 223,486 B serialized; peak RSS 2.4 GB (4.7 GB with the 2^19 segment).

Relayer aggregation (`risc0-spike aggregate N`): one aggregator guest
`env::verify`s each client receipt (composition) and commits a digest over all
journals; output is a succinct receipt.

| Batch | Aggregate | Per tx | Peak RSS |
|---:|---:|---:|---:|
| 2 | 15.3 s | 7.6 s | 1.4 GB |
| 8 | 51.0 s | 6.4 s | 4.7 GB |

Groth16 wrap of the batch-of-8 aggregate (`risc0-spike groth16 8`, run through
`docker-shim/`): **19.5 s**, 256-byte seal, peak RSS 7.7 GB. Fixed per batch,
so the relayer's CPU cost for a batch of 8 is (51.0 + 19.5) / 8 = **8.8 s per tx**,
falling toward the 6.4 s/tx aggregation cost as batches grow.

## Interpreted actions (kernel native, action from a module)

The kernel stays native (grounding, commitments, whitelists, hashing); only the
entry action (`craft_wood_pick`) runs from a module, through an interpreter in
the guest. Every configuration gives the native kernel's exact public data
(`interp-check`, and asserted again by each zkVM host), and the `evil` module is
rejected by the kernel's whitelist check after replay.

Modules: plain craft-basics is 376 B (RV32IM image) / 324 B (wasm); `bloat`
adds 256 never-called functions, 31 KB / 29 KB. Module hashing uses each zkVM's
bulk hash (`Compress::hash_long`), and module bytes are deserialized as one
buffer (`ModuleBytes`); without these, a 31 KB module cost 0.9 M (Jolt) and
0.65 M (RISC Zero) cycles to hash, plus ~15 cycles/byte to decode.

Cycles inside the guest (Jolt: trace length incl. virtual instructions,
`jolt-spike analyze-modules`; RISC Zero: user cycles, `risc0-spike execute-modules`):

| Configuration | Module | Jolt total | RISC Zero total | load (J / R0) | interpret (J / R0) | hash module (J / R0) |
|---|---:|---:|---:|---:|---:|---:|
| native, no module | - | 121 k | 89 k | - | - | - |
| RV32IM | 376 B | **160 k** | **104 k** | 25 k / 0.9 k | 3.9 k / 3.5 k | 7 k / 1.6 k |
| wasmi eager | 324 B | 305 k | 251 k | 165 k / 149 k | 6.7 k / 5.7 k | 7 k / 1.5 k |
| wasmi lazy-translation | 324 B | 279 k | 229 k | 115 k / 107 k | 31 k / 25 k | 7 k / 1.5 k |
| wasmi lazy | 324 B | 271 k | 219 k | 98 k / 90 k | 37 k / 32 k | 7 k / 1.5 k |
| RV32IM | 31 KB | **674 k** | **210 k** | 22 k / 24 k | 3.9 k / 3.5 k | 496 k / 35 k |
| wasmi eager | 29 KB | 11.6 M | 10.1 M | 11.0 M / 9.9 M | 6.7 k / 5.7 k | 462 k / 33 k |
| wasmi lazy-translation | 29 KB | 3.6 M | 3.4 M | 3.0 M / 3.2 M | 31 k / 25 k | 462 k / 33 k |
| wasmi lazy | 29 KB | 1.16 M | 0.70 M | 0.49 M / 0.48 M | 37 k / 32 k | 462 k / 33 k |

(Lazy modes translate on first call, so their translation shows under
"interpret". RV32IM load is zeroing/copying 64 KiB of module memory; in RISC
Zero fresh memory is already zero.)

Proving, ZK on, 32 threads (Jolt: `prove-module`, 2 runs each within 1%;
RISC Zero: `client-module`, core + succinct compression):

| Configuration | Module | Jolt prove | Jolt proof | RISC Zero client total | RISC Zero peak RSS |
|---|---:|---:|---:|---:|---:|
| native, no module | - | 1.37 s | 81 KB | 12.4 s | 2.4 GB |
| **RV32IM** | 376 B | **1.70 s** | 82 KB | **12.1 s** | 2.3 GB |
| wasmi eager | 324 B | 2.49 s | 86 KB | 21.1 s | 4.7 GB |
| wasmi lazy-translation | 324 B | 2.41 s | 86 KB | 21.1 s | 4.7 GB |
| wasmi lazy | 324 B | 2.39 s | 86 KB | 21.2 s | 4.7 GB |
| **RV32IM** | 31 KB | **3.25 s** | 86 KB | **21.0 s** | 4.7 GB |
| wasmi lazy | 29 KB | 4.5 s | 89 KB | 41.5 s | 9.4 GB |
| wasmi lazy-translation | 29 KB | 9.5 s | 90 KB | 165 s | 9.4 GB |
| wasmi eager | 29 KB | 21.7 s | 93 KB | not run (10 M cycles, 12 segments) | - |

RISC Zero proves in power-of-two segments, so its time steps: anything up to
2^18 cycles (native, RV32IM plain) costs the same 12 s; up to 2^19, 21 s.
Jolt's prover peaked at 0.5-4 GB across these runs (the larger figures include
the first guest compile and the 2^24 preprocessing, 15 s once per program).

Takeaways:

- RV32IM interpretation is close to free for small modules: +25% in Jolt,
  nothing measurable in RISC Zero. Executing the action itself costs ~3.5-4 k
  cycles in either zkVM.
- wasmi's cost is loading, not executing: ~100-165 k cycles even for a 324 B
  module, and 0.5-11 M for 29 KB depending on mode. Lazy is clearly the right
  mode; eager is ruled out for anything but tiny modules.
- Past the interpreter, module size costs hashing: about 16 cycles/byte in Jolt
  (BLAKE3 inline) vs about 1 cycle/byte in RISC Zero (SHA-256 accelerator).
  Keeping per-transaction modules small matters more in Jolt.
- Not measured: cross-module calls (the RV32IM interpreter would need one
  memory per module instance and kernel-mediated calls; wasmi gives separate
  instances directly), and wasmi fuel metering.

## SP1 6.10, excluded

The SP1 prover and SDK do not use the VEIL zero-knowledge wrapper that ships in
its `slop` tree; Core/Compressed proofs are not hiding, and ZK comes only from
the Groth16/Plonk wrap. That fails the "client proof is ZK before the wrap"
requirement.

## Conclusions so far

- Client side, Jolt is about 9x faster than RISC Zero (1.37 s vs 12.4 s on 32
  threads) and about 10x faster than v1 (about 14 s), with a ~150 MB prover. RISC Zero's
  client cost is close to v1's.
- Relayer side, only RISC Zero has a working path today: composition at about
  6 s per tx on this CPU (GPU proving would cut this), then one 19.5 s Groth16
  per batch yielding a 256-byte proof, so the blob carries one proof plus each
  tx's public data.
- A Jolt-client / RISC Zero-relayer hybrid needs a Jolt ZK verifier inside the
  aggregator guest; going by Jolt's own recursion figures that is tens of
  millions of cycles per proof. At the ~30 k cycles/s RISC Zero proved here
  (2^18 cycles in 8.9 s), ~79 M cycles is about 45 minutes of CPU per client
  proof, against 6 s/tx for native composition. Jolt-native recursion with a
  wrap, or a cheaper verifier, is the open question for choosing Jolt.

- Interpreting actions is affordable: an RV32IM interpreter in the guest adds
  about 25% in Jolt (1.37 s to 1.70 s) and nothing measurable in RISC Zero for
  a small module. wasmi costs 1.7x (Jolt) / 1.7x (RISC Zero) for a tiny module
  and grows quickly with module size, because of loading.

## Reproduce

```sh
# Jolt: needs the jolt CLI from the pinned commit on PATH
cargo install --git https://github.com/a16z/jolt --rev 6d800d22dfe66968f520e95eed3dfdf52e87d562 --locked --root ~/.cache/zkvm-spike/jolt-cli jolt
cd jolt && cargo build --release
PATH=~/.cache/zkvm-spike/jolt-cli/bin:$PATH ../memguard.sh 16 -- ./target/release/jolt-spike analyze
PATH=~/.cache/zkvm-spike/jolt-cli/bin:$PATH ../memguard.sh 16 -- ./target/release/jolt-spike prove blake3 3

# RISC Zero: needs rzup (curl -L https://risczero.com/install | bash && rzup install)
cd risc0 && cargo build --release
../memguard.sh 16 -- ./target/release/risc0-spike client 8
../memguard.sh 14 3 -- ./target/release/risc0-spike aggregate 8

# Groth16 without a Docker daemon: unpack risczero/risc0-groth16-prover:v2026-10-20.1
# (e.g. skopeo copy to dir:, untar layers) into ~/.cache/zkvm-spike/r0-groth16-rootfs,
# then put docker-shim/ first on PATH.
PATH=$PWD/docker-shim:$PATH ../memguard.sh 14 3 -- ./target/release/risc0-spike groth16 8

# Interpreted actions: build the modules first (needs the wasm32-unknown-unknown
# and riscv32im-unknown-none-elf targets on stable), then check natively.
../module/build.sh
cd ../interp-check && cargo run --release
cd ../jolt && PATH=~/.cache/zkvm-spike/jolt-cli/bin:$PATH RUST_LOG=info ../memguard.sh 16 -- ./target/release/jolt-spike analyze-modules
PATH=~/.cache/zkvm-spike/jolt-cli/bin:$PATH ../memguard.sh 18 -- ./target/release/jolt-spike prove-module rv32 plain 2
cd ../risc0 && ../memguard.sh 14 -- ./target/release/risc0-spike execute-modules
../memguard.sh 16 -- ./target/release/risc0-spike client-module wasm-lazy plain 1
```
