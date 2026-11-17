// Shared by the client guest and the host (included with #[path]), so both
// compute the same digests. In the guest, Impl uses the SHA-256 accelerator.
use dobj_kernel::{Compress, Digest};
use risc0_zkvm::sha::{Digest as R0Digest, Impl, Sha256, SHA256_INIT};

pub struct ShaCompress;

impl Compress for ShaCompress {
    fn compress(left: &Digest, right: &Digest) -> Digest {
        let out = Impl::compress(
            &SHA256_INIT,
            &R0Digest::from_bytes(*left),
            &R0Digest::from_bytes(*right),
        );
        out.as_bytes().try_into().unwrap()
    }

    /// One SHA-256 over all of `data`, so the accelerator takes every block in
    /// one call, then bound to the domain and length.
    fn hash_long(domain: u32, data: &[u8]) -> Digest {
        let mut tag = [0u8; 32];
        tag[..4].copy_from_slice(&domain.to_le_bytes());
        tag[4..12].copy_from_slice(&(data.len() as u64).to_le_bytes());
        let body: Digest = Impl::hash_bytes(data).as_bytes().try_into().unwrap();
        Self::compress(&tag, &body)
    }
}
