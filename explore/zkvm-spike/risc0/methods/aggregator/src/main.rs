//! Verifies a batch of client receipts (resolved as composition assumptions)
//! and commits the client image id plus a digest over every client journal.
use risc0_zkvm::{
    guest::env,
    sha::{Digest, Impl, Sha256},
};

fn main() {
    let (client_image_id, journals): (Digest, Vec<Vec<u8>>) = env::read();
    let mut batch = Vec::with_capacity(32 * journals.len());
    for journal in &journals {
        env::verify(client_image_id, journal).unwrap();
        batch.extend_from_slice(Impl::hash_bytes(journal).as_bytes());
    }
    let batch_digest = *Impl::hash_bytes(&batch);
    env::commit(&(client_image_id, journals.len() as u32, batch_digest));
}
