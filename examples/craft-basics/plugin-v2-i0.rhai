fn FindLog(ctx) -> (Context, Log) {
    let log = Log{};
    ctx.create(log);
    let work = vdf(3, log);
    return (ctx, log);
}

fn CraftWood(ctx, log: Log, key) -> (Context, Wood) {
    ctx.delete(log);
    let wood = Wood{key: key};
    ctx.create(wood);
    assert(lt_eq_u256(wood, 9007199254740992 << 64*3));
    return (ctx, wood);
}

fn CraftSticks(ctx, wood: Wood) -> (Context, Stick, Stick) {
    ctx.delete(wood);
    let (stick_a, stick_b) = (Stick{}, Stick{});
    ctx.create(stick_a);
    ctx.create(stick_b);
    return (ctx, stick_a, stick_b);
}

fn CraftWoodPick(ctx, wood: Wood, stick: Stick) -> (Context, WoodPick) {
    ctx.delete(wood);
    ctx.delete(stick);
    let pick = WoodPick{durability: 100}
    ctx.create(pick);
    return (ctx, pick);
}

fn use_pick(ctx, pick: Obj, key, vdf_iters) -> (Obj) {
    assert(pick.durability > 0);
    pick.durability -= 1;
    pick.key = key;
    let work = vdf(vdf_iters, pick);
}

fn UseWoodPick(ctx, wood_pick: WoodPick) -> (Context, WoodPick) {
    let wood_pick' = use_pick(wood_pick, 10);
    ctx.mutate(wood_pick, wood_pick');
    return (ctx, wood_pick');
}

fn MineStoneWithWoodPick(ctx, pick: WoodPick) -> (Context, WoodPick, Stone) {
    let pick' = ctx.subaction(|ctx| UseWoodPick(ctx, pick));
    let stone = ctx.create(Stone{});
    return (ctx, pick', stone);
}

fn CraftStonePick(ctx, stone: Stone, stick: Stick) -> (Context, StonePick) {
    ctx.delete(stone);
    ctx.delete(stick);
    let pick = ctx.create(StonePick{durability: 200});
    return (ctx, pick);
}

fn UseStonePick(ctx, stone_pick: StonePick) -> (Context, StonePick) {
    let stone_pick' = use_pick(stone_pick, 5);
    ctx.mutate(stone_pick, stone_pick');
    return (ctx, stone_pick');
}

fn MineStoneWithStonePick(ctx, pick: StonePick) -> (Context, StonePick, Stone) {
    let pick' = ctx.subaction(|ctx| UseStonePick(ctx, pick));
    let stone = ctx.create(Stone{});
    return (ctx, pick', stone);
}

struct Context {
    ops: Vec<Operation>,
    state_header: StateHeader,
}

impl Context {
    fn new(state_header: StateHeader) -> Self {
        Self {
            ops: Vec::new();
            state_header,
        }
    }
    fn create(&mut self, o) -> Object {
        self.ops.push(Operation::Event(Event::Create(o)));
        o
    }
    fn delete(&mut self, o) {
        self.ops.push(Operation::Event(Event::Delete(o)));
    }
    fn mutate(&mut self, o, o') {
        self.ops.push(Operation::Event(Event::Mutate(o, o')));
    }
    fn subaction(&mut self, f: fn(Context) -> T) -> T {
        let ctx = Context { ops: Vec::new(), state_header: self.state_header };
        let res = f(ctx);
        self.ops.push(Operation::Subaction(Action{ops: ctx.ops}));
        res
    }
}

fn IsLog(ctx, ctx', o, o') {
    let ctx0 = Context::new(state_header)
    match (o, o') {
        (None, o) => {
            verify_trace(FindLog, (ctx), (ctx', o))
        },
        (o, None) => {
            verify_trace(CraftWood, (ctx, o, _), (ctx', _))
        },
        (o, o') => {}
    }
}

fn IsWood(ctx, ctx', o, o') {
    match (o, o') {
        (None, o) => {
            verify_trace(CraftWood, (ctx, _, _), (ctx', o))
        },
        (o, None) => {
            verify_trace(CraftSticks, (ctx, o), (ctx', _, _)) ||
                verify_trace(CraftWoodPick, (ctx, o, _), (ctx', _))
        },
        (o, o') => {}
    }
}

enum Event {
    Create(Object),
    Delete(Object),
    Mutate(Object, Object),
}

enum Operation {
    Event(Event),
    Subaction(Action),
}

struct Action {
    ops: Vec<Operation>,
}

fn InputsGrounded(input_indices: Array<Int>, created: Array<Untyped>) -> Array<Untyped> {
    let inputs = Array::new();
    for index in input_indices {
        inputs.push(created[index]);
    }
    return inputs;
}

fn ReplayAction(state_header: StateHeader, ops: Vec<Operation>) -> (Set<Object>, Set<Object>) {
    let created = Set::new();
    let nullifiers = Set::new();
    let ctx = Context::new(state_header);
    let ctx' = Context{ops, state_header}
    for op in ops {
        match op {
            Operation::Event(event) => {
                let (type_guard, o, o') = match event {
                    Event::Create(o) => {
                        created.insert(o);
                        (o.type, None, Some(o))
                    }
                    Event::Delete(o) => {
                        assert(created.contains(o) || state_header.created.contains(o));
                        nullifeirs.insert(o);
                        (o.type, Some(o), None)
                    }
                    Event::Mutate(o, o') => {
                        assert(created.contains(o) || state_header.created.contains(o));
                        created.insert(o');
                        nullifeirs.insert(o);
                        assert(o.type == o'.type);
                        assert(o.stable_identifier == o'.stable_identifier);
                        (o.type, Some(o), Some(o'))
                    }
                };
                type_guard(ctx, ctx', o, o');
            }
            Operation::Subaction(action) => {
                let (created', nullifeirs') = ReplayAction(state_header, action.ops);
                created.extend(created');
                nullifiers.extend(nullifiers');
            }
        }
    }
    return (created, nullifiers);
}

fn FinalizeTx(state_header: StateHeader, input_indices: Array<Int>, ops: Vec<Operation>) {
    let (created, nullifiers) = ReplayAction(state_header, ops)
}
