/*! This module contains assertions we make about runtime properties of core library methods.
 * Some properties that influence codegen:
 *  - Whether the method has been redefined since boot
 *  - Whether the C method can yield to the GC
 *  - Whether the C method makes any method calls
 *
 * For Ruby methods, many of these properties can be inferred through analyzing the
 * bytecode, but for C methods we resort to annotation and validation in debug builds.
 */

use crate::cruby::*;
use std::collections::HashMap;
use std::ffi::c_void;
use crate::hir::{Function, InsnId, BlockId, Insn, Invariant};
use crate::hir_type::{types, Type};

pub struct Annotations {
    cfuncs: HashMap<*mut c_void, FnProperties>,
}

/// Runtime behaviors of C functions that implement a Ruby method
#[derive(Clone, Copy)]
pub struct FnProperties {
    /// Whether it's possible for the function to yield to the GC
    pub no_gc: bool,
    /// Whether it's possible for the function to make a ruby call
    pub leaf: bool,
    /// What Type the C function returns
    pub return_type: Type,
    /// Whether it's legal to remove the call if the result is unused
    pub elidable: bool,
    pub inline: &'static dyn Fn(&mut Function, BlockId, InsnId, Vec<InsnId>) -> Option<InsnId>,
}

impl Annotations {
    /// Query about properties of a C method
    pub fn get_cfunc_properties(&self, method: *const rb_callable_method_entry_t) -> Option<FnProperties> {
        let fn_ptr = unsafe {
            if VM_METHOD_TYPE_CFUNC != get_cme_def_type(method) {
                return None;
            }
            get_mct_func(get_cme_def_body_cfunc(method.cast()))
        };
        self.cfuncs.get(&fn_ptr).copied()
    }
}

fn annotate_c_method(props_map: &mut HashMap<*mut c_void, FnProperties>, class: VALUE, method_name: &'static str, props: FnProperties) {
    // Lookup function pointer of the C method
    let fn_ptr = unsafe {
        // TODO(alan): (side quest) make rust methods and clean up glue code for rb_method_cfunc_t and
        // rb_method_definition_t.
        let method_id = rb_intern2(method_name.as_ptr().cast(), method_name.len() as _);
        let method = rb_method_entry_at(class, method_id);
        assert!(!method.is_null(), "Could not find {}#{method_name}", get_class_name(class));
        // ME-to-CME cast is fine due to identical layout
        debug_assert_eq!(VM_METHOD_TYPE_CFUNC, get_cme_def_type(method.cast()));
        get_mct_func(get_cme_def_body_cfunc(method.cast()))
    };

    props_map.insert(fn_ptr, props);
}

struct TmpBlock {
}

impl TmpBlock {
    fn push_insn(&mut self, insn: Insn

    pub fn coerce_to_fixnum(&mut self, block: BlockId, val: InsnId, state: InsnId) -> InsnId {
        if self.is_a(val, types::Fixnum) { return val; }
        return self.push_insn(block, Insn::GuardType { val, guard_type: types::Fixnum, state });
    }
}

fn no_inline(_fun: &mut Function, _block: BlockId, _state: InsnId, _args: Vec<InsnId>) -> Option<InsnId> {
    None
}

fn kernel_itself(_fun: &mut Function, _block: BlockId, _state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [self_val] = args[..] else { return None };
    Some(self_val)
}

fn fixnum_add(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumAdd { left, right, state }))
}

fn fixnum_sub(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumSub { left, right, state }))
}

fn fixnum_mul(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumMult { left, right, state }))
}

fn fixnum_div(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumDiv { left, right, state }))
}

fn fixnum_mod(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumMod { left, right, state }))
}

fn fixnum_eq(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumEq { left, right }))
}

fn basicobject_neq(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    // BasicObject#!= is a more general function but for now we only inline if we can do fixnum comparison.
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    // For opt_neq, the interpreter checks that both neq and eq are unchanged.
    fun.push_insn(block, Insn::PatchPoint(Invariant::BOPRedefined { klass: INTEGER_REDEFINED_OP_FLAG, bop: BOP_EQ }));
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumNeq { left, right }))
}

fn fixnum_lt(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumLt { left, right }))
}

fn fixnum_le(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumLe { left, right }))
}

fn fixnum_gt(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumGt { left, right }))
}

fn fixnum_ge(fun: &mut Function, block: BlockId, state: InsnId, args: Vec<InsnId>) -> Option<InsnId> {
    let [left, right] = args[..] else { return None };
    if !fun.arguments_likely_fixnums(left, right, state) { return None; }
    let left = fun.coerce_to_fixnum(block, left, state);
    let right = fun.coerce_to_fixnum(block, right, state);
    Some(fun.push_insn(block, Insn::FixnumGe { left, right }))
}

/// Gather annotations. Run this right after boot since the annotations
/// are about the stock versions of methods.
pub fn init() -> Annotations {
    let cfuncs = &mut HashMap::new();

    macro_rules! annotate {
        ($module:ident, $method_name:literal, $return_type:expr, $inline:expr, $($properties:ident),*) => {
            #[allow(unused_mut)]
            let mut props = FnProperties { no_gc: false, leaf: false, elidable: false, return_type: $return_type, inline: $inline };
            $(
                props.$properties = true;
            )*
            annotate_c_method(cfuncs, unsafe { $module }, $method_name, props);
        }
    }

    annotate!(rb_mKernel, "itself", types::BasicObject, &kernel_itself, no_gc, leaf);
    annotate!(rb_cString, "bytesize", types::Fixnum, &no_inline, no_gc, leaf);
    annotate!(rb_cModule, "name", types::StringExact.union(types::NilClassExact), &no_inline, no_gc, leaf);
    annotate!(rb_cModule, "===", types::BoolExact, &no_inline, no_gc, leaf);
    annotate!(rb_cArray, "length", types::Fixnum, &no_inline, no_gc, leaf, elidable);
    annotate!(rb_cArray, "size", types::Fixnum, &no_inline, no_gc, leaf, elidable);
    annotate!(rb_cBasicObject, "!=", types::BoolExact, &basicobject_neq,);
    annotate!(rb_cInteger, "+", types::IntegerExact, &fixnum_add,);
    annotate!(rb_cInteger, "-", types::IntegerExact, &fixnum_sub,);
    annotate!(rb_cInteger, "*", types::IntegerExact, &fixnum_mul,);
    annotate!(rb_cInteger, "/", types::IntegerExact, &fixnum_div,);
    annotate!(rb_cInteger, "%", types::IntegerExact, &fixnum_mod,);
    annotate!(rb_cInteger, "==", types::BoolExact, &fixnum_gt,);
    annotate!(rb_cInteger, "<", types::BoolExact, &fixnum_lt,);
    annotate!(rb_cInteger, "<=", types::BoolExact, &fixnum_le,);
    annotate!(rb_cInteger, ">", types::BoolExact, &fixnum_gt,);
    annotate!(rb_cInteger, ">=", types::BoolExact, &fixnum_ge,);

    Annotations {
        cfuncs: std::mem::take(cfuncs)
    }
}
