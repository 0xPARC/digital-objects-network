use dobj_kernel::{run_tx, TxRequest};
use risc0_zkvm::guest::env;

#[path = "../../../sha_compress.rs"]
mod sha_compress;

/// Input and journal are postcard bytes: the default word-per-byte serde
/// encoding makes reading the request dominate the guest's cycles.
fn main() {
    let start = env::cycle_count();
    let mut len: u32 = 0;
    env::read_slice(core::slice::from_mut(&mut len));
    let mut frame = vec![0u8; len as usize];
    env::read_slice(&mut frame);
    let request: TxRequest = postcard::from_bytes(&frame).unwrap();
    let after_read = env::cycle_count();
    let public = run_tx::<sha_compress::ShaCompress>(&request);
    let after_run = env::cycle_count();
    env::commit_slice(&postcard::to_allocvec(&public).unwrap());
    let after_commit = env::cycle_count();
    eprintln!(
        "guest cycles: read={} run_tx={} commit={}",
        after_read - start,
        after_run - after_read,
        after_commit - after_run
    );
}
