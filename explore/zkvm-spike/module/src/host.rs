//! Host calls. Numbers and the field layout must match `dobj_kernel::module::abi`.

#[repr(C)]
pub struct Field {
    pub key: u32,
    pub value: u64,
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::Field;

    #[link(wasm_import_module = "dobj")]
    extern "C" {
        fn obj_class(handle: u32) -> u32;
        fn obj_get(handle: u32, key: u32) -> u64;
        fn obj_delete(handle: u32);
        fn obj_create(class: u32, fields: *const Field, num_fields: u32) -> u32;
        fn obj_mutate(handle: u32, fields: *const Field, num_fields: u32) -> u32;
    }

    pub fn class(handle: u32) -> u32 {
        unsafe { obj_class(handle) }
    }
    pub fn get(handle: u32, key: u32) -> u64 {
        unsafe { obj_get(handle, key) }
    }
    pub fn delete(handle: u32) {
        unsafe { obj_delete(handle) }
    }
    pub fn create(class: u32, fields: &[Field]) -> u32 {
        unsafe { obj_create(class, fields.as_ptr(), fields.len() as u32) }
    }
    pub fn mutate(handle: u32, fields: &[Field]) -> u32 {
        unsafe { obj_mutate(handle, fields.as_ptr(), fields.len() as u32) }
    }
}

#[cfg(target_arch = "riscv32")]
mod imp {
    use super::Field;

    const OBJ_CLASS: u32 = 1;
    const OBJ_GET: u32 = 2;
    const OBJ_DELETE: u32 = 3;
    const OBJ_CREATE: u32 = 4;
    const OBJ_MUTATE: u32 = 5;

    #[inline(always)]
    fn ecall(number: u32, arg0: u32, arg1: u32, arg2: u32) -> (u32, u32) {
        let result0;
        let result1;
        unsafe {
            core::arch::asm!(
                "ecall",
                inlateout("a0") arg0 => result0,
                inlateout("a1") arg1 => result1,
                in("a2") arg2,
                in("a7") number,
                options(nostack),
            );
        }
        (result0, result1)
    }

    pub fn class(handle: u32) -> u32 {
        ecall(OBJ_CLASS, handle, 0, 0).0
    }
    pub fn get(handle: u32, key: u32) -> u64 {
        let (low, high) = ecall(OBJ_GET, handle, key, 0);
        (low as u64) | ((high as u64) << 32)
    }
    pub fn delete(handle: u32) {
        ecall(OBJ_DELETE, handle, 0, 0);
    }
    pub fn create(class: u32, fields: &[Field]) -> u32 {
        ecall(OBJ_CREATE, class, fields.as_ptr() as u32, fields.len() as u32).0
    }
    pub fn mutate(handle: u32, fields: &[Field]) -> u32 {
        ecall(OBJ_MUTATE, handle, fields.as_ptr() as u32, fields.len() as u32).0
    }
}

pub use imp::*;
