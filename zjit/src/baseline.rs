//! Baseline compiler. It compiles each YARV instruction into a call to the
//! interpreter's own handler for the instruction (see
//! rb_zjit_baseline_exec_insn() in vm.c), so it supports every instruction.
//! The handlers keep the PC and SP in the control frame, so the JIT code only
//! follows the PC to the next instruction and returns Qundef to let the
//! interpreter take over whenever it can't.

#![allow(non_upper_case_globals)]

use std::collections::HashMap;
use crate::asm::CodeBlock;
use crate::backend::lir::{self, Assembler, CFP, EC, Opnd, Target, asm_comment};
use crate::cruby::*;
use crate::hir;
use crate::stats::CompileError;
use crate::virtualmem::CodePtr;

unsafe extern "C" {
    fn rb_zjit_baseline_exec_insn(ec: EcPtr, cfp: CfpPtr, opcode: u32) -> VALUE;
}

/// Compile an ISEQ with the baseline compiler
pub fn gen_baseline(cb: &mut CodeBlock, iseq: IseqPtr) -> Result<CodePtr, CompileError> {
    let iseq_size = unsafe { get_iseq_encoded_size(iseq) };
    let pc_at = |insn_idx: u32| Opnd::const_ptr(unsafe { rb_iseq_pc_at_idx(iseq, insn_idx) });
    let opcode_at = |insn_idx: u32| unsafe {
        rb_iseq_bare_opcode_at_pc(iseq, rb_iseq_pc_at_idx(iseq, insn_idx)) as u32
    };
    let edge = |target| Target::Block(Box::new(lir::BranchEdge { target, args: vec![] }));

    let mut asm = Assembler::new();
    asm.new_block(hir::BlockId(0), true, 0);
    let label = asm.new_label("baseline_entry");
    asm.write_label(label);
    asm.frame_setup(&[]);

    // Create a block for each instruction and one for exiting to the interpreter
    let mut blocks = HashMap::new();
    let mut insn_idx = 0;
    while insn_idx < iseq_size {
        blocks.insert(insn_idx, new_block(&mut asm, insn_idx));
        insn_idx += insn_len(opcode_at(insn_idx) as usize);
    }
    let exit_block = new_block(&mut asm, iseq_size);

    // Jump to the instruction where the interpreter entered the frame
    let mut entries: Vec<u32> = unsafe { iseq.params() }.opt_table_slice().iter().map(|pc| pc.as_u32()).collect();
    entries.sort();
    entries.dedup();
    for (i, &entry) in entries.iter().enumerate() {
        let pc = asm.load(Opnd::mem(64, CFP, RUBY_OFFSET_CFP_PC));
        asm.cmp(pc, pc_at(entry));
        asm.push_insn(lir::Insn::Je(edge(blocks[&entry])));
        let next = if i + 1 < entries.len() {
            new_block(&mut asm, 0)
        } else {
            exit_block
        };
        asm.jmp(edge(next));
        asm.set_current_block(next);
    }

    let mut insn_idx = 0;
    while insn_idx < iseq_size {
        let opcode = opcode_at(insn_idx);
        let next_idx = insn_idx + insn_len(opcode as usize);
        asm.set_current_block(blocks[&insn_idx]);
        asm_comment!(asm, "{}: {}", insn_idx, insn_name(opcode as usize));

        // throw returns to the interpreter without a value, so let the interpreter run it
        if opcode == YARVINSN_throw {
            asm.jmp(edge(exit_block));
            insn_idx = next_idx;
            continue;
        }

        // Run the instruction. If it was leave, return the value.
        let ret = asm.ccall(rb_zjit_baseline_exec_insn as *const u8, vec![EC, CFP, Opnd::UImm(opcode.into())]);
        asm.cmp(ret, Qundef.into());
        let continue_block = new_block(&mut asm, insn_idx);
        asm.push_insn(lir::Insn::Je(edge(continue_block)));
        let return_block = new_block(&mut asm, insn_idx);
        asm.jmp(edge(return_block));
        asm.set_current_block(return_block);
        asm.frame_teardown(&[]);
        asm.cret(ret);

        // Follow the PC to the next instruction or the jump target
        asm.set_current_block(continue_block);
        if let Some(target_idx) = jump_target(iseq, opcode, insn_idx, next_idx) {
            let pc = asm.load(Opnd::mem(64, CFP, RUBY_OFFSET_CFP_PC));
            asm.cmp(pc, pc_at(target_idx));
            asm.push_insn(lir::Insn::Je(edge(blocks[&target_idx])));
            let fallthrough_block = new_block(&mut asm, insn_idx);
            asm.jmp(edge(fallthrough_block));
            asm.set_current_block(fallthrough_block);
        }
        match blocks.get(&next_idx) {
            Some(&next_block) => {
                let pc = asm.load(Opnd::mem(64, CFP, RUBY_OFFSET_CFP_PC));
                asm.cmp(pc, pc_at(next_idx));
                asm.push_insn(lir::Insn::Je(edge(next_block)));
                asm.jmp(edge(exit_block));
            }
            None => asm.jmp(edge(exit_block)),
        }
        insn_idx = next_idx;
    }

    // Let the interpreter continue from the current PC
    asm.set_current_block(exit_block);
    asm.frame_teardown(&[]);
    asm.cret(Qundef.into());

    let (code_ptr, gc_offsets) = asm.compile(cb)?;
    assert!(gc_offsets.is_empty());
    Ok(code_ptr)
}

/// Create a block, ordered by the instruction index, that starts with a label
fn new_block(asm: &mut Assembler, insn_idx: u32) -> lir::BlockId {
    let block = asm.new_block(hir::BlockId(insn_idx), false, insn_idx as usize);
    let current_block = asm.current_block().id;
    asm.set_current_block(block);
    let label = asm.new_label(&format!("insn_{insn_idx}"));
    asm.write_label(label);
    asm.set_current_block(current_block);
    block
}

/// Return the instruction index a branch instruction may jump to
fn jump_target(iseq: IseqPtr, opcode: u32, insn_idx: u32, next_idx: u32) -> Option<u32> {
    let pc = unsafe { rb_iseq_pc_at_idx(iseq, insn_idx) };
    let offset_operand = match opcode {
        YARVINSN_branchunless | YARVINSN_jump | YARVINSN_branchif | YARVINSN_branchnil
        | YARVINSN_branchunless_without_ints | YARVINSN_jump_without_ints
        | YARVINSN_branchif_without_ints | YARVINSN_branchnil_without_ints => 1,
        YARVINSN_opt_new => 2,
        _ => return None,
    };
    let offset = unsafe { *pc.add(offset_operand) }.as_i64();
    Some((next_idx as i64 + offset) as u32)
}
