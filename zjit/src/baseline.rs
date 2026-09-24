//! Baseline compiler. It generates a direct call to the interpreter's function
//! for each YARV instruction (see rb_zjit_baseline_* in vm.c), so it supports
//! every instruction. The functions keep the PC and SP in the control frame,
//! and C functions handle anything but continuing to the next instruction.

#![allow(non_upper_case_globals)]

use crate::asm::CodeBlock;
use crate::cruby::*;
use crate::virtualmem::CodePtr;

unsafe extern "C" {
    fn rb_zjit_baseline_insn_func(opcode: i32) -> *const u8;
    fn rb_zjit_baseline_frame_changed(ec: EcPtr, cfp: CfpPtr, next_cfp: CfpPtr) -> VALUE;
    fn rb_zjit_baseline_exec(ec: EcPtr, cfp: CfpPtr) -> VALUE;
}

/// Compile an ISEQ with the baseline compiler
#[cfg(target_arch = "aarch64")]
pub fn gen_baseline(cb: &mut CodeBlock, iseq: IseqPtr) -> Option<CodePtr> {
    use crate::asm::{Label, arm64::*};
    use crate::invariants::track_no_trace_point_assumption;
    use crate::payload::{get_or_create_iseq_payload, IseqCodePtrs, IseqStatus, IseqVersion};
    const EC: A64Opnd = X19;
    const CFP: A64Opnd = X20;
    const SP: A64Opnd = X31;

    let pc_at = |insn_idx: u32| unsafe { rb_iseq_pc_at_idx(iseq, insn_idx) } as u64;
    let opcode_at = |insn_idx: u32| unsafe { rb_iseq_bare_opcode_at_pc(iseq, rb_iseq_pc_at_idx(iseq, insn_idx)) } as u32;
    let load_imm = |cb: &mut CodeBlock, reg, value: u64| {
        movz(cb, reg, A64Opnd::new_uimm(value & 0xffff), 0);
        for shift in [16, 32, 48] {
            movk(cb, reg, A64Opnd::new_uimm((value >> shift) & 0xffff), shift);
        }
    };
    let call = |cb: &mut CodeBlock, func: *const u8| {
        mov(cb, C_ARG_REGS[0], EC);
        mov(cb, C_ARG_REGS[1], CFP);
        load_imm(cb, X16, func as u64);
        blr(cb, X16);
    };
    // Branch to a label if the condition holds. With None, branch always.
    let branch = |cb: &mut CodeBlock, cond: Option<u8>, label: Label| {
        let Some(cond) = cond else {
            cb.label_ref(label, 4, |cb, src_addr, dst_addr| {
                // +1 since src_addr is after the instruction
                b(cb, InstructionOffset::from_insns(((dst_addr - src_addr) / 4 + 1) as i32));
                Ok(())
            });
            return;
        };
        cb.label_ref(label, 8, move |cb, src_addr, dst_addr| {
            let offset = (dst_addr - src_addr) / 4 + 2;
            if bcond_offset_fits_bits(offset) {
                bcond(cb, cond, InstructionOffset::from_insns(offset as i32));
                nop(cb);
            } else {
                bcond(cb, Condition::inverse(cond), InstructionOffset::from_insns(2));
                b(cb, InstructionOffset::from_insns((offset - 1) as i32));
            }
            Ok(())
        });
    };
    // Branch to a label if the condition holds for comparing cfp->pc with the PC of insn_idx
    let branch_pc = |cb: &mut CodeBlock, cond: u8, insn_idx: u32, label: Label| {
        ldur(cb, X9, A64Opnd::new_mem(64, CFP, RUBY_OFFSET_CFP_PC));
        load_imm(cb, X10, pc_at(insn_idx));
        cmp(cb, X9, X10);
        branch(cb, Some(cond), label);
    };

    // Instruction functions don't fire events, so let the interpreter run trace_* instructions
    if unsafe { rb_zjit_iseq_tracing_currently_enabled() } {
        return None;
    }
    let iseq_size = unsafe { get_iseq_encoded_size(iseq) };
    let mut insn_idx = 0;
    while insn_idx < iseq_size {
        let opcode = unsafe { rb_iseq_opcode_at_pc(iseq, rb_iseq_pc_at_idx(iseq, insn_idx)) } as u32;
        if (YARVINSN_trace_nop..YARVINSN_zjit_getinstancevariable).contains(&opcode) {
            return None;
        }
        insn_idx += insn_len(opcode as usize);
    }

    let insn_labels: Vec<Label> = (0..iseq_size).map(|idx| cb.new_label(format!("insn_{idx}"))).collect();
    let exec_label = cb.new_label("exec".into());
    let return_label = cb.new_label("return".into());

    // Save EC and CFP in callee-saved registers
    let start_ptr = cb.get_write_ptr();
    stp_pre(cb, X29, X30, A64Opnd::new_mem(128, SP, -16));
    mov(cb, X29, SP);
    stp_pre(cb, X19, X20, A64Opnd::new_mem(128, SP, -16));
    mov(cb, EC, C_ARG_REGS[0]);
    mov(cb, CFP, C_ARG_REGS[1]);

    // Jump to the instruction where the frame starts
    let mut entries: Vec<u32> = unsafe { iseq.params() }.opt_table_slice().iter().map(|pc| pc.as_u32()).collect();
    entries.push(0);
    entries.sort();
    entries.dedup();
    for entry in entries {
        branch_pc(cb, Condition::EQ, entry, insn_labels[entry as usize]);
    }
    branch(cb, None, exec_label);

    let mut patch_ptrs = vec![];
    let mut insn_idx = 0;
    while insn_idx < iseq_size {
        let opcode = opcode_at(insn_idx);
        let next_idx = insn_idx + insn_len(opcode as usize);
        let continue_label = cb.new_label(format!("continue_{insn_idx}"));

        // Call the instruction function, and call rb_zjit_baseline_frame_changed() if it returns non-NULL.
        // TracePoint patches the start of it to jump to exec_label.
        cb.write_label(insn_labels[insn_idx as usize]);
        patch_ptrs.push(cb.get_write_ptr());
        call(cb, unsafe { rb_zjit_baseline_insn_func(opcode as i32) });
        cb.label_ref(continue_label, 4, |cb, src_addr, dst_addr| {
            cbz(cb, X0, InstructionOffset::from_insns(((dst_addr - src_addr) / 4 + 1) as i32));
            Ok(())
        });
        mov(cb, C_ARG_REGS[2], X0);
        call(cb, rb_zjit_baseline_frame_changed as *const u8);
        cmp(cb, X0, A64Opnd::new_uimm(Qundef.as_u64()));
        branch(cb, Some(Condition::NE), return_label);

        // Follow the PC if the instruction may jump
        cb.write_label(continue_label);
        let target_idx = |operand: usize| (next_idx as i64 + unsafe { *rb_iseq_pc_at_idx(iseq, insn_idx).add(operand) }.as_i64()) as u32;
        match opcode {
            YARVINSN_jump | YARVINSN_jump_without_ints => branch(cb, None, insn_labels[target_idx(1) as usize]),
            YARVINSN_branchif | YARVINSN_branchunless | YARVINSN_branchnil
            | YARVINSN_branchif_without_ints | YARVINSN_branchunless_without_ints | YARVINSN_branchnil_without_ints => {
                branch_pc(cb, Condition::NE, next_idx, insn_labels[target_idx(1) as usize]);
            }
            YARVINSN_opt_new => branch_pc(cb, Condition::NE, next_idx, insn_labels[target_idx(2) as usize]),
            YARVINSN_opt_case_dispatch => branch_pc(cb, Condition::NE, next_idx, exec_label),
            _ => {}
        }
        insn_idx = next_idx;
    }

    // Run the rest of the frame in C
    cb.write_label(exec_label);
    call(cb, rb_zjit_baseline_exec as *const u8);
    cb.write_label(return_label);
    ldp_post(cb, X19, X20, A64Opnd::new_mem(128, SP, 16));
    ldp_post(cb, X29, X30, A64Opnd::new_mem(128, SP, 16));
    ret(cb, A64Opnd::None);

    if cb.has_dropped_bytes() {
        cb.clear_labels();
        return None;
    }
    let exec_ptr = cb.resolve_label(exec_label);
    cb.link_labels().ok()?;
    unsafe { rb_jit_icache_invalidate(start_ptr.raw_ptr(cb) as _, cb.get_write_ptr().raw_ptr(cb) as _) };

    // Let TracePoint patch the instructions to run in C, which runs trace_* instructions
    let mut version = IseqVersion::new(iseq);
    unsafe { version.as_mut() }.status = IseqStatus::Compiled(IseqCodePtrs { start_ptr, jit_entry_ptrs: vec![] });
    get_or_create_iseq_payload(iseq).versions.push(version);
    for patch_ptr in patch_ptrs {
        track_no_trace_point_assumption(patch_ptr, exec_ptr, version);
    }
    Some(start_ptr)
}

#[cfg(not(target_arch = "aarch64"))]
pub fn gen_baseline(_cb: &mut CodeBlock, _iseq: IseqPtr) -> Option<CodePtr> {
    None
}
