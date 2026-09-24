//! Baseline compiler. It doesn't generate any code. Instead, it runs the
//! interpreter's own instruction handlers with rb_zjit_baseline_exec() in vm.c,
//! so it supports every instruction and feature of the interpreter.

use crate::cruby::*;
use crate::payload::get_or_create_iseq_payload;

unsafe extern "C" {
    fn rb_zjit_baseline_exec(ec: EcPtr, cfp: CfpPtr) -> VALUE;
}

/// A cache of the opcode of each instruction in iseq_encoded. See rb_zjit_baseline_slot_t in vm.c.
pub type BaselineSlot = std::ffi::c_uint;

/// Compile an ISEQ with the baseline compiler
pub fn gen_baseline(iseq: IseqPtr) -> *const u8 {
    let payload = get_or_create_iseq_payload(iseq);
    if payload.baseline_sled.is_empty() {
        let iseq_size = unsafe { get_iseq_encoded_size(iseq) } as usize;
        payload.baseline_sled = vec![0; iseq_size];
    }
    rb_zjit_baseline_exec as *const u8
}

/// Return the sled of an ISEQ compiled by gen_baseline()
#[unsafe(no_mangle)]
pub extern "C" fn rb_zjit_baseline_sled(iseq: IseqPtr) -> *mut BaselineSlot {
    get_or_create_iseq_payload(iseq).baseline_sled.as_mut_ptr()
}
