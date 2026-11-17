use dobj_kernel::{fixture, run_tx, Compress, Digest};

struct Toy;
impl Compress for Toy {
    fn compress(left: &Digest, right: &Digest) -> Digest {
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = left[i].rotate_left(3) ^ right[i].wrapping_mul(31) ^ (i as u8);
        }
        out
    }
}

#[test]
fn craft_wood_pick_runs() {
    let request = fixture::craft_wood_pick_request::<Toy>(1);
    let public = run_tx::<Toy>(&request);
    assert_eq!(public.nullifiers.len(), 2);
    assert_eq!(public.created.len(), 1);
}

#[test]
#[should_panic(expected = "input not in state root")]
fn rejects_ungrounded_input() {
    let mut request = fixture::craft_wood_pick_request::<Toy>(1);
    request.inputs[0].object.fields.push((9, 9));
    run_tx::<Toy>(&request);
}
