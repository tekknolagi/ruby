//! Cranelift-based backend for ZJIT.
//!
//! This module replaces the custom LIR pipeline with Cranelift for the main
//! function body compilation. Entry/exit trampolines and function stubs still
//! use the existing LIR Assembler.

use std::sync::Arc;

use cranelift_codegen::ir::{
    types, AbiParam, Block, Function, InstBuilder, MemFlags, Signature, UserFuncName, Value,
};
use cranelift_codegen::ir::immediates::Offset32;
use cranelift_codegen::isa::{self, TargetIsa, CallConv};
use cranelift_codegen::settings;
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};

use crate::asm::CodeBlock;
use crate::cruby::*;
use crate::hir::SideExitReason;
use crate::stats::CompileError;
use crate::virtualmem::CodePtr;

/// Side-exit context: captures interpreter state to restore on deoptimization.
/// Unlike the LIR SideExit which stores LIR operands, this stores Cranelift Values
/// that are live at the point of the side exit.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CLSideExit {
    /// PC to restore (as a raw pointer constant)
    pub pc: *const u8,
    /// Number of stack values
    pub stack_size: usize,
    /// Number of local values
    pub locals_size: usize,
}

/// Info about a side exit block and the values it needs to save
pub struct SideExitBlockInfo {
    pub block: Block,
    /// The Cranelift Values for stack slots, captured at creation time
    stack_vals: Vec<Value>,
    /// The Cranelift Values for locals, captured at creation time
    local_vals: Vec<Value>,
    /// PC value
    pc_val: Value,
    /// Reason for the exit
    reason: SideExitReason,
}

/// Cranelift-based code builder for ZJIT function bodies.
pub struct CraneliftBuilder {
    /// The Cranelift function being built
    func: Function,
    /// Context for the FunctionBuilder
    builder_ctx: FunctionBuilderContext,
    /// Target ISA
    isa: Arc<dyn TargetIsa>,

    /// Cranelift Variable for EC (execution context pointer)
    pub ec_var: Variable,
    /// Cranelift Variable for CFP (control frame pointer)
    pub cfp_var: Variable,
    /// Cranelift Variable for SP (stack pointer)
    pub sp_var: Variable,

    /// Side exit blocks, keyed by a dedup key. Each side exit block saves
    /// VM state (PC, stack, locals) and returns Qundef.
    side_exit_blocks: Vec<SideExitBlockInfo>,

    /// Value pool: Ruby VALUEs stored in a heap-allocated Vec.
    /// Generated code loads from this pool via indirection instead of
    /// embedding raw VALUE pointers in machine code.
    pub value_pool: Vec<VALUE>,

    /// Next variable index for Cranelift Variables
    next_var: usize,
}

impl CraneliftBuilder {
    /// Create a new CraneliftBuilder for a function with the ZJIT calling convention:
    /// `(EC: i64, CFP: i64) -> i64`
    pub fn new() -> Self {
        let shared_builder = settings::builder();
        let shared_flags = settings::Flags::new(shared_builder);
        let isa = isa::lookup(target_lexicon::Triple::host())
            .expect("Failed to look up target ISA")
            .finish(shared_flags)
            .expect("Failed to finish ISA");

        let call_conv = isa.default_call_conv();
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(types::I64)); // EC
        sig.params.push(AbiParam::new(types::I64)); // CFP
        sig.returns.push(AbiParam::new(types::I64)); // VALUE return

        let func = Function::with_name_signature(UserFuncName::default(), sig);

        let ec_var = Variable::from_u32(0);
        let cfp_var = Variable::from_u32(1);
        let sp_var = Variable::from_u32(2);

        CraneliftBuilder {
            func,
            builder_ctx: FunctionBuilderContext::new(),
            isa,
            ec_var,
            cfp_var,
            sp_var,
            side_exit_blocks: Vec::new(),
            value_pool: Vec::new(),
            next_var: 3, // 0=EC, 1=CFP, 2=SP
        }
    }

    /// Allocate a fresh Cranelift Variable
    pub fn new_variable(&mut self) -> Variable {
        let var = Variable::from_u32(self.next_var as u32);
        self.next_var += 1;
        var
    }

    /// Build the function using a closure that receives the FunctionBuilder.
    /// This method handles the FunctionBuilder lifecycle.
    pub fn build<F>(&mut self, f: F)
    where
        F: FnOnce(&mut FunctionBuilder, &Arc<dyn TargetIsa>, &mut Vec<SideExitBlockInfo>, &mut Vec<VALUE>, Variable, Variable, Variable, &mut usize),
    {
        let mut builder = FunctionBuilder::new(&mut self.func, &mut self.builder_ctx);

        // Declare EC, CFP, SP variables
        builder.declare_var(self.ec_var, types::I64);
        builder.declare_var(self.cfp_var, types::I64);
        builder.declare_var(self.sp_var, types::I64);

        f(
            &mut builder,
            &self.isa,
            &mut self.side_exit_blocks,
            &mut self.value_pool,
            self.ec_var,
            self.cfp_var,
            self.sp_var,
            &mut self.next_var,
        );

        builder.finalize();
    }

    /// Compile the built function and copy the machine code into the CodeBlock.
    /// Returns the start CodePtr and GC offsets (empty since we use value pool).
    pub fn compile(self, cb: &mut CodeBlock) -> Result<(CodePtr, Vec<CodePtr>), CompileError> {
        let mut ctx = Context::for_function(self.func);

        ctx.compile(&*self.isa, &mut Default::default())
            .map_err(|e| {
                eprintln!("Cranelift compilation error: {e:?}");
                CompileError::CraneliftError
            })?;

        let code = ctx.compiled_code().unwrap();
        let code_bytes = code.code_buffer();

        if cb.has_dropped_bytes() {
            return Err(CompileError::OutOfMemory);
        }

        let start_ptr = cb.get_write_ptr();
        cb.write_bytes(code_bytes);

        if cb.has_dropped_bytes() {
            return Err(CompileError::OutOfMemory);
        }

        // No GC offsets needed — we use a value pool instead of embedding VALUEs
        Ok((start_ptr, vec![]))
    }
}

/// Helper to create a C call signature with the given number of i64 arguments
/// and an i64 return value.
pub fn make_ccall_sig(call_conv: CallConv, num_args: usize) -> Signature {
    let mut sig = Signature::new(call_conv);
    for _ in 0..num_args {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// Helper to create a void C call signature (no return value).
pub fn make_ccall_sig_void(call_conv: CallConv, num_args: usize) -> Signature {
    let mut sig = Signature::new(call_conv);
    for _ in 0..num_args {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig
}

/// Emit a side exit block that saves VM state and returns Qundef.
///
/// This is called after all main blocks are emitted.
pub fn emit_side_exit_block(
    builder: &mut FunctionBuilder,
    info: &SideExitBlockInfo,
    cfp_var: Variable,
    sp_var: Variable,
) {
    builder.switch_to_block(info.block);

    let cfp = builder.use_var(cfp_var);
    let sp = builder.use_var(sp_var);

    // Store PC to [CFP + RUBY_OFFSET_CFP_PC]
    builder.ins().store(MemFlags::trusted(), info.pc_val, cfp, Offset32::new(RUBY_OFFSET_CFP_PC));

    // Store SP to [CFP + RUBY_OFFSET_CFP_SP] = SP + stack.len() * SIZEOF_VALUE
    let sp_offset = builder.ins().iconst(types::I64, (info.stack_vals.len() * SIZEOF_VALUE) as i64);
    let new_sp = builder.ins().iadd(sp, sp_offset);
    builder.ins().store(MemFlags::trusted(), new_sp, cfp, Offset32::new(RUBY_OFFSET_CFP_SP));

    // Write stack values to interpreter stack: SP[idx] = val
    for (idx, &val) in info.stack_vals.iter().enumerate() {
        let offset = (idx * SIZEOF_VALUE) as i32;
        builder.ins().store(MemFlags::trusted(), val, sp, Offset32::new(offset));
    }

    // Write locals
    let local_size = info.local_vals.len();
    for (idx, &val) in info.local_vals.iter().enumerate() {
        let ep_offset = crate::codegen::local_size_and_idx_to_ep_offset(local_size, idx);
        let byte_offset = -(ep_offset + 1) * SIZEOF_VALUE_I32;
        builder.ins().store(MemFlags::trusted(), val, sp, Offset32::new(byte_offset));
    }

    // Return Qundef
    let qundef = builder.ins().iconst(types::I64, Qundef.as_i64());
    builder.ins().return_(&[qundef]);

    builder.seal_block(info.block);
}

/// Build a side exit block. Returns the Block but does NOT switch to it or emit code.
/// The block body is emitted later via emit_side_exit_block().
pub fn create_side_exit(
    builder: &mut FunctionBuilder,
    side_exit_blocks: &mut Vec<SideExitBlockInfo>,
    pc: *const u8,
    stack_vals: Vec<Value>,
    local_vals: Vec<Value>,
    reason: SideExitReason,
) -> Block {
    let block = builder.create_block();
    let pc_val = builder.ins().iconst(types::I64, pc as i64);

    side_exit_blocks.push(SideExitBlockInfo {
        block,
        stack_vals,
        local_vals,
        pc_val,
        reason,
    });

    block
}

/// Perform an indirect C function call and return the result.
pub fn call_c_function(
    builder: &mut FunctionBuilder,
    isa: &Arc<dyn TargetIsa>,
    fptr: *const u8,
    args: &[Value],
) -> Value {
    let sig = make_ccall_sig(isa.default_call_conv(), args.len());
    let sig_ref = builder.import_signature(sig);
    let addr = builder.ins().iconst(types::I64, fptr as i64);
    let call = builder.ins().call_indirect(sig_ref, addr, args);
    builder.inst_results(call)[0]
}

/// Perform an indirect C function call that returns void.
pub fn call_c_function_void(
    builder: &mut FunctionBuilder,
    isa: &Arc<dyn TargetIsa>,
    fptr: *const u8,
    args: &[Value],
) {
    let sig = make_ccall_sig_void(isa.default_call_conv(), args.len());
    let sig_ref = builder.import_signature(sig);
    let addr = builder.ins().iconst(types::I64, fptr as i64);
    builder.ins().call_indirect(sig_ref, addr, args);
}
