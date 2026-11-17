//! The craft-basics plugin written against the kernel: plain functions that
//! compute, plus a declarative whitelist per class.

use alloc::{vec, vec::Vec};
use serde::{Deserialize, Serialize};

use crate::{ClassId, ClassPolicy, Compress, Ctx, FnId, Object};

pub const LOG: ClassId = ClassId(0);
pub const WOOD: ClassId = ClassId(1);
pub const STICK: ClassId = ClassId(2);
pub const STONE: ClassId = ClassId(3);
pub const WOOD_PICK: ClassId = ClassId(4);

pub const CRAFT_STICKS: FnId = FnId(2);
pub const CRAFT_WOOD_PICK: FnId = FnId(3);
pub const USE_WOOD_PICK: FnId = FnId(4);
pub const MINE_STONE_WITH_WOOD_PICK: FnId = FnId(5);
pub const FIND_LOG: FnId = FnId(0);
pub const CRAFT_WOOD: FnId = FnId(1);

pub const DURABILITY: u32 = 1;
pub const KEY: u32 = 2;

const LOG_POLICY: ClassPolicy = ClassPolicy {
    create: &[FIND_LOG],
    delete: &[CRAFT_WOOD],
    mutate: &[],
};
const WOOD_POLICY: ClassPolicy = ClassPolicy {
    create: &[CRAFT_WOOD],
    delete: &[CRAFT_STICKS, CRAFT_WOOD_PICK],
    mutate: &[],
};
const STICK_POLICY: ClassPolicy = ClassPolicy {
    create: &[CRAFT_STICKS],
    delete: &[CRAFT_WOOD_PICK],
    mutate: &[],
};
const STONE_POLICY: ClassPolicy = ClassPolicy {
    create: &[MINE_STONE_WITH_WOOD_PICK],
    delete: &[],
    mutate: &[],
};
const WOOD_PICK_POLICY: ClassPolicy = ClassPolicy {
    create: &[CRAFT_WOOD_PICK],
    delete: &[],
    mutate: &[USE_WOOD_PICK],
};

pub fn policy(class: ClassId) -> &'static ClassPolicy {
    match class {
        LOG => &LOG_POLICY,
        WOOD => &WOOD_POLICY,
        STICK => &STICK_POLICY,
        STONE => &STONE_POLICY,
        WOOD_PICK => &WOOD_PICK_POLICY,
        _ => panic!("unknown class"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Entry {
    CraftSticks,
    CraftWoodPick,
    MineStoneWithWoodPick,
}

pub fn dispatch<C: Compress>(ctx: &mut Ctx<C>, entry: Entry, inputs: Vec<Object>, args: &[u64]) {
    let mut inputs = inputs.into_iter();
    let mut next_input = || inputs.next().expect("missing input");
    match entry {
        Entry::CraftSticks => {
            craft_sticks(ctx, next_input());
        }
        Entry::CraftWoodPick => {
            let wood = next_input();
            let stick = next_input();
            craft_wood_pick(ctx, wood, stick);
        }
        Entry::MineStoneWithWoodPick => {
            mine_stone_with_wood_pick(ctx, next_input(), args[0]);
        }
    }
}

pub fn craft_sticks<C: Compress>(ctx: &mut Ctx<C>, wood: Object) -> (Object, Object) {
    ctx.call(CRAFT_STICKS, |ctx| {
        assert_eq!(wood.class, WOOD);
        ctx.delete(wood);
        (ctx.create(STICK, vec![]), ctx.create(STICK, vec![]))
    })
}

pub fn craft_wood_pick<C: Compress>(ctx: &mut Ctx<C>, wood: Object, stick: Object) -> Object {
    ctx.call(CRAFT_WOOD_PICK, |ctx| {
        assert_eq!(wood.class, WOOD);
        assert_eq!(stick.class, STICK);
        ctx.delete(wood);
        ctx.delete(stick);
        ctx.create(WOOD_PICK, vec![(DURABILITY, 100)])
    })
}

pub fn use_wood_pick<C: Compress>(ctx: &mut Ctx<C>, pick: Object, key: u64) -> Object {
    ctx.call(USE_WOOD_PICK, |ctx| {
        assert_eq!(pick.class, WOOD_PICK);
        let durability = pick.get(DURABILITY).expect("pick has durability");
        assert!(durability > 0);
        let used = pick.with_field(DURABILITY, durability - 1).with_field(KEY, key);
        ctx.mutate(pick, used)
    })
}

pub fn mine_stone_with_wood_pick<C: Compress>(ctx: &mut Ctx<C>, pick: Object, key: u64) -> (Object, Object) {
    ctx.call(MINE_STONE_WITH_WOOD_PICK, |ctx| {
        let pick = use_wood_pick(ctx, pick, key);
        (pick, ctx.create(STONE, vec![]))
    })
}
