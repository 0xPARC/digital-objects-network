// Shared by the interpreted client guests (included with #[path]).
use dobj_kernel::module::Probe;
use risc0_zkvm::guest::env;

/// Reads one postcard-encoded frame: a u32 length, then the bytes.
pub fn read_frame<T: serde::de::DeserializeOwned>() -> T {
    let mut length: u32 = 0;
    env::read_slice(core::slice::from_mut(&mut length));
    let mut frame = vec![0u8; length as usize];
    env::read_slice(&mut frame);
    postcard::from_bytes(&frame).unwrap()
}

#[derive(Default)]
pub struct CycleProbe {
    started: Vec<(&'static str, u64)>,
    finished: Vec<(&'static str, u64)>,
}

impl Probe for CycleProbe {
    fn begin(&mut self, label: &'static str) {
        self.started.push((label, env::cycle_count()));
    }
    fn end(&mut self, label: &'static str) {
        let position = self.started.iter().rposition(|(started, _)| *started == label).unwrap();
        let (_, start) = self.started.remove(position);
        self.finished.push((label, env::cycle_count() - start));
    }
}

impl CycleProbe {
    pub fn report(&self) {
        for (label, cycles) in &self.finished {
            eprintln!("MARK {label} {cycles}");
        }
    }
}
