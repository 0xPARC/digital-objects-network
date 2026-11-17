//! RV32IM interpreter for module images.
//!
//! A module image is the flat memory image of a module linked at `BASE`: an
//! export table first, then code and data. The module gets `MEMORY_BYTES` of
//! memory starting at `BASE`, the stack at the top; every fetch, load and
//! store is bounds-checked, so a malformed module can trap but not escape.
//! `ecall` is a host call, numbered in a7 (see `dobj_kernel::module::abi`).

#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};
use dobj_kernel::module::{abi, HostObjects, ModuleInstance, Trap};

pub const BASE: u32 = 0x0001_0000;
pub const MEMORY_BYTES: u32 = 0x0001_0000;
const EXPORT_BYTES: usize = 32;
const EXPORT_NAME_BYTES: usize = 28;
/// Outside module memory: a jump here means the export returned.
const RETURN_ADDRESS: u32 = 0xffff_fff0;
const MAX_STEPS: u64 = 50_000_000;

struct Export {
    name: [u8; EXPORT_NAME_BYTES],
    entry: u32,
}

pub struct Rv32Module {
    memory: Vec<u8>,
    exports: Vec<Export>,
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Trap> {
    bytes
        .get(offset..offset + 4)
        .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
        .ok_or(Trap("truncated module image"))
}

impl Rv32Module {
    pub fn load(image: &[u8]) -> Result<Self, Trap> {
        if image.len() > MEMORY_BYTES as usize {
            return Err(Trap("module image too large"));
        }
        if image.get(0..4) != Some(b"DOBJ") {
            return Err(Trap("bad module magic"));
        }
        let num_exports = read_u32(image, 4)? as usize;
        let mut exports = Vec::with_capacity(num_exports);
        for index in 0..num_exports {
            let offset = 8 + index * EXPORT_BYTES;
            let name = image
                .get(offset..offset + EXPORT_NAME_BYTES)
                .ok_or(Trap("truncated export table"))?;
            exports.push(Export {
                name: name.try_into().unwrap(),
                entry: read_u32(image, offset + EXPORT_NAME_BYTES)?,
            });
        }
        let mut memory = vec![0u8; MEMORY_BYTES as usize];
        memory[..image.len()].copy_from_slice(image);
        Ok(Self { memory, exports })
    }

    fn find_export(&self, name: &str) -> Result<u32, Trap> {
        let wanted = name.as_bytes();
        self.exports
            .iter()
            .find(|export| {
                let length = export.name.iter().position(|byte| *byte == 0).unwrap_or(EXPORT_NAME_BYTES);
                &export.name[..length] == wanted
            })
            .map(|export| export.entry)
            .ok_or(Trap("unknown export"))
    }

    fn offset(&self, address: u32, length: u32) -> Result<usize, Trap> {
        let offset = address.wrapping_sub(BASE);
        if offset >= MEMORY_BYTES || length > MEMORY_BYTES - offset {
            return Err(Trap("memory access out of bounds"));
        }
        Ok(offset as usize)
    }

    fn load_bytes<const N: usize>(&self, address: u32) -> Result<[u8; N], Trap> {
        let offset = self.offset(address, N as u32)?;
        Ok(self.memory[offset..offset + N].try_into().unwrap())
    }

    fn store_bytes(&mut self, address: u32, bytes: &[u8]) -> Result<(), Trap> {
        let offset = self.offset(address, bytes.len() as u32)?;
        self.memory[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    fn slice(&self, address: u32, length: u32) -> Result<&[u8], Trap> {
        let offset = self.offset(address, length)?;
        Ok(&self.memory[offset..offset + length as usize])
    }

    fn host_call(&mut self, regs: &mut [u32; 32], host: &mut HostObjects) -> Result<(), Trap> {
        let (arg0, arg1, arg2) = (regs[10], regs[11], regs[12]);
        let field_bytes = |count: u32| count.checked_mul(abi::FIELD_BYTES as u32).ok_or(Trap("field count"));
        match regs[17] {
            abi::OBJ_CLASS => regs[10] = host.class(arg0)?,
            abi::OBJ_GET => {
                let value = host.get(arg0, arg1)?;
                regs[10] = value as u32;
                regs[11] = (value >> 32) as u32;
            }
            abi::OBJ_DELETE => host.delete(arg0)?,
            abi::OBJ_CREATE => {
                let fields = self.slice(arg1, field_bytes(arg2)?)?;
                regs[10] = host.create(arg0, fields)?;
            }
            abi::OBJ_MUTATE => {
                let fields = self.slice(arg1, field_bytes(arg2)?)?;
                regs[10] = host.mutate(arg0, fields)?;
            }
            _ => return Err(Trap("unknown host call")),
        }
        Ok(())
    }

    fn run(&mut self, entry: u32, args: &[u32], host: &mut HostObjects) -> Result<u32, Trap> {
        if args.len() > 8 {
            return Err(Trap("too many arguments"));
        }
        let mut regs = [0u32; 32];
        regs[1] = RETURN_ADDRESS;
        regs[2] = BASE + MEMORY_BYTES;
        regs[10..10 + args.len()].copy_from_slice(args);
        let mut pc = entry;

        for _ in 0..MAX_STEPS {
            if pc == RETURN_ADDRESS {
                return Ok(regs[10]);
            }
            if pc % 4 != 0 {
                return Err(Trap("misaligned pc"));
            }
            let instruction = u32::from_le_bytes(self.load_bytes::<4>(pc)?);
            let opcode = instruction & 0x7f;
            let rd = ((instruction >> 7) & 31) as usize;
            let funct3 = (instruction >> 12) & 7;
            let rs1 = regs[((instruction >> 15) & 31) as usize];
            let rs2 = regs[((instruction >> 20) & 31) as usize];
            let funct7 = instruction >> 25;
            let imm_i = ((instruction as i32) >> 20) as u32;
            let mut next_pc = pc.wrapping_add(4);
            let mut result: Option<u32> = None;

            match opcode {
                0x37 => result = Some(instruction & 0xffff_f000),
                0x17 => result = Some(pc.wrapping_add(instruction & 0xffff_f000)),
                0x6f => {
                    let imm_j = ((((instruction as i32) >> 31) << 20) as u32)
                        | (instruction & 0x000f_f000)
                        | ((instruction >> 9) & 0x800)
                        | ((instruction >> 20) & 0x7fe);
                    result = Some(next_pc);
                    next_pc = pc.wrapping_add(imm_j);
                }
                0x67 => {
                    result = Some(next_pc);
                    next_pc = rs1.wrapping_add(imm_i) & !1;
                }
                0x63 => {
                    let imm_b = ((((instruction as i32) >> 31) << 12) as u32)
                        | ((instruction << 4) & 0x800)
                        | ((instruction >> 20) & 0x7e0)
                        | ((instruction >> 7) & 0x1e);
                    let taken = match funct3 {
                        0 => rs1 == rs2,
                        1 => rs1 != rs2,
                        4 => (rs1 as i32) < (rs2 as i32),
                        5 => (rs1 as i32) >= (rs2 as i32),
                        6 => rs1 < rs2,
                        7 => rs1 >= rs2,
                        _ => return Err(Trap("illegal branch")),
                    };
                    if taken {
                        next_pc = pc.wrapping_add(imm_b);
                    }
                }
                0x03 => {
                    let address = rs1.wrapping_add(imm_i);
                    result = Some(match funct3 {
                        0 => self.load_bytes::<1>(address)?[0] as i8 as i32 as u32,
                        1 => i16::from_le_bytes(self.load_bytes::<2>(address)?) as i32 as u32,
                        2 => u32::from_le_bytes(self.load_bytes::<4>(address)?),
                        4 => self.load_bytes::<1>(address)?[0] as u32,
                        5 => u16::from_le_bytes(self.load_bytes::<2>(address)?) as u32,
                        _ => return Err(Trap("illegal load")),
                    });
                }
                0x23 => {
                    let imm_s = ((((instruction as i32) >> 25) << 5) as u32) | ((instruction >> 7) & 0x1f);
                    let address = rs1.wrapping_add(imm_s);
                    match funct3 {
                        0 => self.store_bytes(address, &[rs2 as u8])?,
                        1 => self.store_bytes(address, &(rs2 as u16).to_le_bytes())?,
                        2 => self.store_bytes(address, &rs2.to_le_bytes())?,
                        _ => return Err(Trap("illegal store")),
                    }
                }
                0x13 => {
                    let shift = (instruction >> 20) & 31;
                    result = Some(match funct3 {
                        0 => rs1.wrapping_add(imm_i),
                        2 => ((rs1 as i32) < (imm_i as i32)) as u32,
                        3 => (rs1 < imm_i) as u32,
                        4 => rs1 ^ imm_i,
                        6 => rs1 | imm_i,
                        7 => rs1 & imm_i,
                        1 if funct7 == 0 => rs1 << shift,
                        5 if funct7 == 0 => rs1 >> shift,
                        5 if funct7 == 0x20 => ((rs1 as i32) >> shift) as u32,
                        _ => return Err(Trap("illegal op-imm")),
                    });
                }
                0x33 if funct7 == 1 => {
                    result = Some(match funct3 {
                        0 => rs1.wrapping_mul(rs2),
                        1 => ((rs1 as i32 as i64 * rs2 as i32 as i64) >> 32) as u32,
                        2 => ((rs1 as i32 as i64 * rs2 as u64 as i64) >> 32) as u32,
                        3 => ((rs1 as u64 * rs2 as u64) >> 32) as u32,
                        4 => match (rs1 as i32, rs2 as i32) {
                            (_, 0) => u32::MAX,
                            (i32::MIN, -1) => i32::MIN as u32,
                            (dividend, divisor) => (dividend / divisor) as u32,
                        },
                        5 => rs1.checked_div(rs2).unwrap_or(u32::MAX),
                        6 => match (rs1 as i32, rs2 as i32) {
                            (dividend, 0) => dividend as u32,
                            (i32::MIN, -1) => 0,
                            (dividend, divisor) => (dividend % divisor) as u32,
                        },
                        _ => rs1.checked_rem(rs2).unwrap_or(rs1),
                    });
                }
                0x33 => {
                    result = Some(match (funct3, funct7) {
                        (0, 0) => rs1.wrapping_add(rs2),
                        (0, 0x20) => rs1.wrapping_sub(rs2),
                        (1, 0) => rs1 << (rs2 & 31),
                        (2, 0) => ((rs1 as i32) < (rs2 as i32)) as u32,
                        (3, 0) => (rs1 < rs2) as u32,
                        (4, 0) => rs1 ^ rs2,
                        (5, 0) => rs1 >> (rs2 & 31),
                        (5, 0x20) => ((rs1 as i32) >> (rs2 & 31)) as u32,
                        (6, 0) => rs1 | rs2,
                        (7, 0) => rs1 & rs2,
                        _ => return Err(Trap("illegal op")),
                    });
                }
                0x0f => {}
                0x73 if instruction == 0x0000_0073 => self.host_call(&mut regs, host)?,
                _ => return Err(Trap("illegal instruction")),
            }

            if let Some(value) = result {
                if rd != 0 {
                    regs[rd] = value;
                }
            }
            pc = next_pc;
        }
        Err(Trap("step limit exceeded"))
    }
}

impl ModuleInstance for Rv32Module {
    fn call(&mut self, export: &str, args: &[u32], host: &mut HostObjects) -> Result<u32, Trap> {
        let entry = self.find_export(export)?;
        self.run(entry, args, host)
    }
}

/// Host-side: turns a module ELF linked at `BASE` into its flat image.
pub fn flatten_elf(elf: &[u8]) -> Vec<u8> {
    let half = |offset: usize| u16::from_le_bytes(elf[offset..offset + 2].try_into().unwrap()) as usize;
    let word = |offset: usize| u32::from_le_bytes(elf[offset..offset + 4].try_into().unwrap()) as usize;
    assert_eq!(&elf[0..4], b"\x7fELF");
    assert_eq!(elf[4], 1, "ELF32 expected");
    let program_headers = word(28);
    let program_header_size = half(42);
    let num_program_headers = half(44);
    let mut image = Vec::new();
    for index in 0..num_program_headers {
        let header = program_headers + index * program_header_size;
        const PT_LOAD: usize = 1;
        if word(header) != PT_LOAD {
            continue;
        }
        let (file_offset, address, file_bytes) = (word(header + 4), word(header + 8), word(header + 16));
        let start = address - BASE as usize;
        if image.len() < start + file_bytes {
            image.resize(start + file_bytes, 0);
        }
        image[start..start + file_bytes].copy_from_slice(&elf[file_offset..file_offset + file_bytes]);
    }
    image
}
