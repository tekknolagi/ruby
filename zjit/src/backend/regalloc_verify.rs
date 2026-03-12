//! Register allocation verifier based on symbolic execution.
//!
//! Inspired by the Cranelift register allocation checker:
//! https://cfallin.org/blog/2021/03/15/cranelift-isel-3/
//!
//! Two verification phases:
//! 1. **Assignment overlap check**: no two overlapping live ranges share a physical register.
//! 2. **Symbolic execution**: track which VReg value is in each physical location and
//!    verify every instruction reads the correct value.

use std::collections::HashMap;
use super::lir::*;

// ============================================================
// Symbolic value lattice
// ============================================================

/// Symbolic value that can reside in a physical location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SymValue {
    /// No information about this location (initial state, or clobbered).
    Unknown,
    /// Contains the value defined by this virtual register.
    VReg(VRegId),
    /// Multiple conflicting values merged at a join point.
    Conflict,
}

impl SymValue {
    /// Meet (join) operation for merging states at control-flow merge points.
    fn meet(self, other: Self) -> Self {
        match (self, other) {
            (SymValue::Unknown, x) | (x, SymValue::Unknown) => x,
            (a, b) if a == b => a,
            _ => SymValue::Conflict,
        }
    }
}

impl std::fmt::Display for SymValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SymValue::Unknown => write!(f, "Unknown"),
            SymValue::VReg(v) => write!(f, "{v}"),
            SymValue::Conflict => write!(f, "Conflict"),
        }
    }
}

// ============================================================
// Physical storage locations
// ============================================================

/// A physical storage location tracked by the verifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Loc {
    /// Physical register, identified by register number.
    Reg(u8),
    /// Stack slot, identified by index.
    Stack(usize),
}

impl std::fmt::Display for Loc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Loc::Reg(n) => write!(f, "Reg({n})"),
            Loc::Stack(n) => write!(f, "Stack[{n}]"),
        }
    }
}

/// Convert a post-allocation operand to a storage location.
fn opnd_to_loc(opnd: &Opnd) -> Option<Loc> {
    match opnd {
        Opnd::Reg(reg) => Some(Loc::Reg(reg.reg_no)),
        Opnd::Mem(Mem { base: MemBase::Stack { stack_idx, .. }, .. }) => Some(Loc::Stack(*stack_idx)),
        Opnd::Mem(Mem { base: MemBase::Reg(reg_no), .. }) => Some(Loc::Reg(*reg_no)),
        _ => None,
    }
}

/// Extract a VRegId from a pre-allocation operand.
fn extract_vreg(opnd: &Opnd) -> Option<VRegId> {
    match opnd {
        Opnd::VReg { idx, .. } => Some(*idx),
        Opnd::Mem(Mem { base: MemBase::VReg(idx), .. }) => Some(*idx),
        _ => None,
    }
}

// ============================================================
// Checker state
// ============================================================

/// Symbolic state at a program point.
#[derive(Clone, Debug)]
struct CheckerState {
    /// Maps each physical location to its symbolic value.
    locs: HashMap<Loc, SymValue>,
    /// Shadow stack for tracking CPush/CPop sequences.
    shadow_stack: Vec<SymValue>,
}

impl CheckerState {
    fn new() -> Self {
        Self {
            locs: HashMap::new(),
            shadow_stack: Vec::new(),
        }
    }

    fn get(&self, loc: Loc) -> SymValue {
        self.locs.get(&loc).copied().unwrap_or(SymValue::Unknown)
    }

    fn set(&mut self, loc: Loc, val: SymValue) {
        self.locs.insert(loc, val);
    }

    /// Merge another state into self using the meet operation.
    fn meet_with(&mut self, other: &CheckerState) {
        // Collect all locations from both states
        let all_locs: Vec<Loc> = self
            .locs
            .keys()
            .chain(other.locs.keys())
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        for loc in all_locs {
            let a = self.get(loc);
            let b = other.get(loc);
            self.set(loc, a.meet(b));
        }
    }
}

// ============================================================
// VReg annotations (captured before allocation rewrite)
// ============================================================

/// Pre-allocation VReg information for a single instruction.
pub struct InsnAnnotation {
    /// VReg read by each input operand position (None if not a VReg).
    input_vregs: Vec<Option<VRegId>>,
    /// VReg written by the output (None if no VReg output).
    output_vreg: Option<VRegId>,
}

/// Capture VReg annotations for all numbered instructions.
///
/// Must be called BEFORE handle_caller_saved_regs / resolve_ssa, while
/// instructions still reference VRegs.
pub fn capture_annotations(asm: &Assembler) -> HashMap<InsnId, InsnAnnotation> {
    let mut map = HashMap::new();

    for block_id in asm.block_order() {
        let block = &asm.basic_blocks[block_id.0];
        for (insn, insn_id) in block.insns.iter().zip(block.insn_ids.iter()) {
            let Some(id) = insn_id else { continue };

            let (input_vregs, output_vreg) = match insn {
                // Mov: dest is write, src is read
                Insn::Mov { dest, src } => {
                    (vec![extract_vreg(src)], extract_vreg(dest))
                }
                // LoadInto: dest is write, opnd is read
                Insn::LoadInto { dest, opnd } => {
                    (vec![extract_vreg(opnd)], extract_vreg(dest))
                }
                // PatchPoint/Label: all operands are SideExit snapshot metadata,
                // not normal reads on the forward path. Skip.
                Insn::PatchPoint { .. } | Insn::Label(_) => {
                    (vec![], None)
                }
                // Branches with only a target (no separate condition operand):
                // operands are SideExit snapshot or block edge args (cleared after
                // resolve_ssa). Skip.
                Insn::Je(_) | Insn::Jne(_) | Insn::Jmp(_) | Insn::Jz(_) |
                Insn::Jnz(_) | Insn::Jb(_) | Insn::Jbe(_) | Insn::Jg(_) |
                Insn::Jge(_) | Insn::Jl(_) | Insn::Jo(_) | Insn::JoMul(_) => {
                    (vec![], None)
                }
                // Joz/Jonz: first operand is the condition (normal read),
                // remaining operands are SideExit/block metadata. Only capture
                // the condition.
                Insn::Joz(opnd, _) | Insn::Jonz(opnd, _) => {
                    (vec![extract_vreg(opnd)], None)
                }
                // Default: opnd_iter = reads, out_opnd = write
                _ => {
                    let inputs: Vec<Option<VRegId>> =
                        insn.opnd_iter().map(|o| extract_vreg(o)).collect();
                    let output = insn.out_opnd().and_then(extract_vreg);
                    (inputs, output)
                }
            };

            map.insert(*id, InsnAnnotation { input_vregs, output_vreg });
        }
    }
    map
}

// ============================================================
// Phase 1: Assignment overlap check
// ============================================================

/// Verify that no two overlapping live intervals share the same physical register.
pub fn verify_assignments(
    intervals: &[Interval],
    assignments: &[Option<Allocation>],
) {
    use crate::backend::current::ALLOC_REGS;

    // Group intervals by their assigned physical register (reg_no).
    let mut by_reg: HashMap<u8, Vec<&Interval>> = HashMap::new();

    for (i, interval) in intervals.iter().enumerate() {
        if interval.range.start.is_none() || interval.range.end.is_none() {
            continue;
        }
        if let Some(alloc) = assignments[i] {
            let reg_no = match alloc {
                Allocation::Reg(n) => ALLOC_REGS[n].reg_no,
                Allocation::Fixed(reg) => reg.reg_no,
                Allocation::Stack(_) => continue, // Stack slots don't conflict with registers
            };
            by_reg.entry(reg_no).or_default().push(interval);
        }
    }

    // For each register, sort intervals by start and check adjacent pairs for overlap.
    for (reg_no, mut intervals) in by_reg {
        intervals.sort_by_key(|iv| iv.range.start());
        for pair in intervals.windows(2) {
            let a = pair[0];
            let b = pair[1];
            // Overlap if a's end > b's start
            if a.range.end() > b.range.start() {
                panic!(
                    "regalloc verify: overlapping assignments for Reg({reg_no}): \
                     v{} ({}..{}) and v{} ({}..{})",
                    a.id, a.range.start(), a.range.end(),
                    b.id, b.range.start(), b.range.end(),
                );
            }
        }
    }

    // Also check stack slots for overlaps.
    let mut by_stack: HashMap<usize, Vec<&Interval>> = HashMap::new();
    for (i, interval) in intervals.iter().enumerate() {
        if interval.range.start.is_none() || interval.range.end.is_none() {
            continue;
        }
        if let Some(Allocation::Stack(slot)) = assignments[i] {
            by_stack.entry(slot).or_default().push(interval);
        }
    }
    for (slot, mut intervals) in by_stack {
        intervals.sort_by_key(|iv| iv.range.start());
        for pair in intervals.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if a.range.end() > b.range.start() {
                panic!(
                    "regalloc verify: overlapping assignments for Stack[{slot}]: \
                     v{} ({}..{}) and v{} ({}..{})",
                    a.id, a.range.start(), a.range.end(),
                    b.id, b.range.start(), b.range.end(),
                );
            }
        }
    }
}

// ============================================================
// Phase 2: Symbolic execution dataflow check
// ============================================================

/// Verify register allocation correctness via symbolic execution.
///
/// Must be called AFTER resolve_ssa (all VRegs rewritten to physical regs,
/// moves inserted at block boundaries and around CCalls).
pub fn verify_dataflow(
    asm: &Assembler,
    annotations: &HashMap<InsnId, InsnAnnotation>,
    alloc_regs: &[Reg],
    assignments: &[Option<Allocation>],
) {
    let block_order = asm.block_order();

    // Compute predecessors.
    let mut predecessors: HashMap<BlockId, Vec<BlockId>> = HashMap::new();
    for &block_id in &block_order {
        let block = &asm.basic_blocks[block_id.0];
        if block.is_dummy() {
            continue;
        }
        for succ in block.successors() {
            predecessors.entry(succ).or_default().push(block_id);
        }
    }

    // State at block exit.
    let mut exit_states: HashMap<BlockId, CheckerState> = HashMap::new();

    for &block_id in &block_order {
        let block = &asm.basic_blocks[block_id.0];
        if block.is_dummy() {
            continue;
        }

        // Initialize state by merging predecessors.
        let mut state = {
            let preds = predecessors.get(&block_id).cloned().unwrap_or_default();
            let pred_states: Vec<&CheckerState> =
                preds.iter().filter_map(|p| exit_states.get(p)).collect();
            if pred_states.is_empty() {
                let mut s = CheckerState::new();
                // Entry block: parameters arrive in calling-convention locations.
                if block.is_entry {
                    for (i, param) in block.parameters.iter().enumerate() {
                        if let Some(vreg) = extract_vreg(param) {
                            let loc = param_to_loc(i);
                            s.set(loc, SymValue::VReg(vreg));
                        }
                    }
                }
                s
            } else {
                let mut s = pred_states[0].clone();
                for ps in &pred_states[1..] {
                    s.meet_with(ps);
                }
                s
            }
        };

        // At block entry (after merging), set block parameter VRegs at their
        // allocated locations. resolve_ssa inserted moves at block boundaries
        // to ensure each parameter's value arrives at its allocated location.
        // Different predecessors may pass different source VRegs (phi inputs),
        // which the meet merges to Conflict. We override with the parameter
        // VReg here because the edge moves define the parameter's value.
        if !block.is_entry {
            for param in &block.parameters {
                if let Some(vreg) = extract_vreg(param) {
                    if let Some(loc) = alloc_to_loc(assignments, vreg) {
                        state.set(loc, SymValue::VReg(vreg));
                    }
                }
            }
        }

        // Process each instruction in the block.
        for (insn, insn_id) in block.insns.iter().zip(block.insn_ids.iter()) {
            process_insn(insn, insn_id, annotations, &mut state, alloc_regs, block_id, asm);
        }

        exit_states.insert(block_id, state);
    }
}

/// Process a single instruction, updating checker state and verifying inputs.
fn process_insn(
    insn: &Insn,
    insn_id: &Option<InsnId>,
    annotations: &HashMap<InsnId, InsnAnnotation>,
    state: &mut CheckerState,
    alloc_regs: &[Reg],
    block_id: BlockId,
    asm: &Assembler,
) {
    // ---- Inserted moves (no InsnId): propagate symbolic values ----
    if insn_id.is_none() {
        match insn {
            Insn::Mov { dest, src } => {
                let val = opnd_to_loc(src)
                    .map(|l| state.get(l))
                    .unwrap_or(SymValue::Unknown);
                if let Some(loc) = opnd_to_loc(dest) {
                    state.set(loc, val);
                }
            }
            Insn::CPush(opnd) => {
                let val = opnd_to_loc(opnd)
                    .map(|l| state.get(l))
                    .unwrap_or(SymValue::Unknown);
                state.shadow_stack.push(val);
            }
            Insn::CPushPair(a, b) => {
                let va = opnd_to_loc(a)
                    .map(|l| state.get(l))
                    .unwrap_or(SymValue::Unknown);
                let vb = opnd_to_loc(b)
                    .map(|l| state.get(l))
                    .unwrap_or(SymValue::Unknown);
                state.shadow_stack.push(va);
                state.shadow_stack.push(vb);
            }
            Insn::CPopInto(opnd) => {
                let val = state.shadow_stack.pop().unwrap_or(SymValue::Unknown);
                if let Some(loc) = opnd_to_loc(opnd) {
                    state.set(loc, val);
                }
            }
            Insn::CPopPairInto(a, b) => {
                // handle_caller_saved_regs does CPushPair(left, right) then
                // CPopPairInto(right, left). Pop is LIFO: top = last pushed
                // (right's value), below = first pushed (left's value).
                // a = right → gets top, b = left → gets below.
                let top = state.shadow_stack.pop().unwrap_or(SymValue::Unknown);
                let below = state.shadow_stack.pop().unwrap_or(SymValue::Unknown);
                if let Some(loc) = opnd_to_loc(a) {
                    state.set(loc, top);
                }
                if let Some(loc) = opnd_to_loc(b) {
                    state.set(loc, below);
                }
            }
            // Other inserted instructions (Label, Comment, PosMarker, etc.) — no-op.
            _ => {}
        }
        return;
    }

    // ---- Original instructions (have InsnId): verify inputs, set outputs ----
    let id = insn_id.unwrap();
    let Some(ann) = annotations.get(&id) else {
        return;
    };

    // CCall: clobber all allocatable registers, then set output.
    if let Insn::CCall { out, .. } = insn {
        for reg in alloc_regs {
            state.set(Loc::Reg(reg.reg_no), SymValue::Unknown);
        }
        if let Some(vreg) = ann.output_vreg {
            if let Some(loc) = opnd_to_loc(out) {
                state.set(loc, SymValue::VReg(vreg));
            }
        }
        return;
    }

    // Extract post-allocation input operands and output operand based on instruction type.
    let (post_inputs, post_output): (Vec<&Opnd>, Option<&Opnd>) = match insn {
        Insn::Mov { dest, src } => (vec![src], Some(dest)),
        Insn::LoadInto { dest, opnd } => (vec![opnd], Some(dest)),
        _ => {
            let inputs: Vec<&Opnd> = insn.opnd_iter().collect();
            let output = insn.out_opnd();
            (inputs, output)
        }
    };

    // Verify each input operand contains the expected VReg value.
    for (i, opnd) in post_inputs.iter().enumerate() {
        if i >= ann.input_vregs.len() {
            break;
        }
        let Some(expected) = ann.input_vregs[i] else {
            continue;
        };
        let Some(loc) = opnd_to_loc(opnd) else {
            continue;
        };
        let actual = state.get(loc);
        if actual != SymValue::VReg(expected) {
            // Dump the post-allocation LIR for the failing block
            eprintln!("=== REGALLOC VERIFY FAILURE ===");
            eprintln!("Block {block_id} instructions:");
            let failing_block = &asm.basic_blocks[block_id.0];
            for (idx, (dump_insn, dump_id)) in failing_block.insns.iter()
                .zip(failing_block.insn_ids.iter()).enumerate()
            {
                let id_str = dump_id.map_or("    ".to_string(), |i| format!("{i:>4}"));
                let marker = if dump_id == insn_id { " <<< FAIL" } else { "" };
                eprintln!("  [{idx:>3}] {id_str} {dump_insn:?}{marker}");
            }
            eprintln!("Checker state at failure:");
            let mut sorted_locs: Vec<_> = state.locs.iter().collect();
            sorted_locs.sort_by_key(|(l, _)| *l);
            for (l, v) in &sorted_locs {
                eprintln!("  {l} = {v}");
            }
            panic!(
                "regalloc verify: block {block_id}, insn {id} ({insn}): \
                 operand {i} expected {expected} but {loc} contains {actual}",
                insn = insn_name(insn),
            );
        }
    }

    // Set the output location to the defined VReg.
    if let Some(vreg) = ann.output_vreg {
        if let Some(out_opnd) = post_output {
            if let Some(loc) = opnd_to_loc(out_opnd) {
                state.set(loc, SymValue::VReg(vreg));
            }
        }
    }
}

/// Convert a parameter index to its calling-convention location.
fn param_to_loc(idx: usize) -> Loc {
    use crate::backend::current::ALLOC_REGS;

    if idx < ALLOC_REGS.len() {
        Loc::Reg(ALLOC_REGS[idx].reg_no)
    } else {
        // Parameters beyond the register count are passed on the stack.
        Loc::Stack(idx - ALLOC_REGS.len())
    }
}

/// Convert a VReg's allocation to a storage location.
fn alloc_to_loc(assignments: &[Option<Allocation>], vreg: VRegId) -> Option<Loc> {
    use crate::backend::current::ALLOC_REGS;

    match assignments.get(vreg.0)? {
        Some(Allocation::Reg(n)) => Some(Loc::Reg(ALLOC_REGS[*n].reg_no)),
        Some(Allocation::Fixed(reg)) => Some(Loc::Reg(reg.reg_no)),
        Some(Allocation::Stack(n)) => Some(Loc::Stack(*n)),
        None => None,
    }
}

// ============================================================
// Re-export the Reg type alias from the current platform
// ============================================================

use crate::backend::current::Reg;

/// Short name for an instruction (for diagnostics).
fn insn_name(insn: &Insn) -> &'static str {
    match insn {
        Insn::Add { .. } => "Add",
        Insn::And { .. } => "And",
        Insn::CCall { .. } => "CCall",
        Insn::Cmp { .. } => "Cmp",
        Insn::Je(_) => "Je",
        Insn::Jmp(_) => "Jmp",
        Insn::Jne(_) => "Jne",
        Insn::Jnz(_) => "Jnz",
        Insn::Jo(_) => "Jo",
        Insn::JoMul(_) => "JoMul",
        Insn::Jonz(_, _) => "Jonz",
        Insn::Joz(_, _) => "Joz",
        Insn::Jz(_) => "Jz",
        Insn::Label(_) => "Label",
        Insn::Load { .. } => "Load",
        Insn::LoadInto { .. } => "LoadInto",
        Insn::Mov { .. } => "Mov",
        Insn::PatchPoint { .. } => "PatchPoint",
        Insn::Store { .. } => "Store",
        Insn::Sub { .. } => "Sub",
        Insn::Test { .. } => "Test",
        _ => "Other",
    }
}
