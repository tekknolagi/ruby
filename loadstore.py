#!/usr/bin/env python3
"""
Load/store optimization with Type-Based Alias Analysis (TBAA).

This is an extension of the basic load/store optimization that adds type
information to objects, allowing the optimizer to understand that stores to
different object types cannot alias each other.

For example, a store to an Array object cannot affect a load from a Hash object,
even if they're at the same offset, because they are different types.
"""
# See LICENSE for license.
import pytest
import re
from typing import Optional, Any, List, Tuple, Dict
from enum import Enum, auto


class ObjectType(Enum):
    """Object types for Type-Based Alias Analysis."""
    UNKNOWN = auto()  # Unknown or generic object
    ARRAY = auto()    # Ruby Array
    HASH = auto()     # Ruby Hash
    STRING = auto()   # Ruby String
    INTEGER = auto()  # Ruby Integer (Fixnum/Bignum)
    FLOAT = auto()    # Ruby Float
    SYMBOL = auto()   # Ruby Symbol
    RANGE = auto()    # Ruby Range
    REGEXP = auto()   # Ruby Regexp


class Value:
    def find(self):
        raise NotImplementedError("abstract")

    def _set_forwarded(self, value):
        raise NotImplementedError("abstract")


class Operation(Value):
    def __init__(self, name: str, args: List[Value]):
        self.name = name
        self.args = args
        self.forwarded = None
        self.info = None
        self.type = ObjectType.UNKNOWN  # Type information for TBAA

    def __repr__(self):
        return (
            f"Operation({self.name}, "
            f"{self.args}, {self.forwarded}, "
            f"{self.info}, type={self.type.name})"
        )

    def find(self) -> Value:
        op = self
        while isinstance(op, Operation):
            next = op.forwarded
            if next is None:
                return op
            op = next
        return op

    def arg(self, index):
        return self.args[index].find()

    def make_equal_to(self, value: Value):
        self.find()._set_forwarded(value)

    def _set_forwarded(self, value: Value):
        self.forwarded = value


class Constant(Value):
    def __init__(self, value: Any):
        self.value = value

    def __repr__(self):
        return f"Constant({self.value})"

    def find(self):
        return self

    def _set_forwarded(self, value: Value):
        assert isinstance(value, Constant) and value.value == self.value


class Block(list):
    def opbuilder(opname: str):
        def wraparg(arg):
            if not isinstance(arg, Value):
                arg = Constant(arg)
            return arg

        def build(self, *args):
            # construct an Operation, wrap the
            # arguments in Constants if necessary
            op = Operation(opname, [wraparg(arg) for arg in args])
            # add it to self, the basic block
            self.append(op)
            return op

        return build

    # a bunch of operations we support
    add = opbuilder("add")
    mul = opbuilder("mul")
    getarg = opbuilder("getarg")
    dummy = opbuilder("dummy")
    lshift = opbuilder("lshift")
    # memory operations
    alloc = opbuilder("alloc")
    load = opbuilder("load")
    store = opbuilder("store")
    alias = opbuilder("alias")
    escape = opbuilder("escape")
    # typed allocation operations for TBAA
    alloc_array = opbuilder("alloc_array")
    alloc_hash = opbuilder("alloc_hash")
    alloc_string = opbuilder("alloc_string")
    alloc_integer = opbuilder("alloc_integer")
    alloc_float = opbuilder("alloc_float")
    alloc_symbol = opbuilder("alloc_symbol")
    alloc_range = opbuilder("alloc_range")
    alloc_regexp = opbuilder("alloc_regexp")


def bb_to_str(bb: Block, varprefix: str = "var"):
    def arg_to_str(arg: Value):
        if isinstance(arg, Constant):
            return str(arg.value)
        else:
            return varnames[arg]

    varnames = {}
    res = []
    for index, op in enumerate(bb):
        var = f"{varprefix}{index}"
        varnames[op] = var
        arguments = ", ".join(arg_to_str(op.arg(i)) for i in range(len(op.args)))
        strop = f"{var} = {op.name}({arguments})"
        res.append(strop)
    return "\n".join(res)


def get_num(op, index=1):
    assert isinstance(op.arg(index), Constant)
    return op.arg(index).value


def eq_value(left: Value | None, right: Value) -> bool:
    if isinstance(left, Constant) and isinstance(right, Constant):
        return left.value == right.value
    return left is right


# Mapping from operation names to object types
ALLOC_TYPE_MAP = {
    "alloc_array": ObjectType.ARRAY,
    "alloc_hash": ObjectType.HASH,
    "alloc_string": ObjectType.STRING,
    "alloc_integer": ObjectType.INTEGER,
    "alloc_float": ObjectType.FLOAT,
    "alloc_symbol": ObjectType.SYMBOL,
    "alloc_range": ObjectType.RANGE,
    "alloc_regexp": ObjectType.REGEXP,
}


def get_object_type(obj: Value) -> ObjectType:
    """Get the type of an object for TBAA."""
    if isinstance(obj, Operation):
        # Check if the object was created with a typed allocation
        if obj.name in ALLOC_TYPE_MAP:
            return ALLOC_TYPE_MAP[obj.name]
        # Return the stored type information
        return obj.type
    return ObjectType.UNKNOWN


def may_alias(obj1: Value, obj2: Value, offset1: int, offset2: int) -> bool:
    """
    Check if two memory locations may alias using TBAA.
    
    Returns True if the two locations might refer to the same memory location.
    """
    # Same object at same offset always aliases
    if obj1 is obj2 and offset1 == offset2:
        return True
    
    # Same object at different offsets does NOT alias
    if obj1 is obj2 and offset1 != offset2:
        return False
    
    # Different objects - check types
    type1 = get_object_type(obj1)
    type2 = get_object_type(obj2)
    
    # If either type is unknown, conservatively assume they may alias
    if type1 == ObjectType.UNKNOWN or type2 == ObjectType.UNKNOWN:
        return True
    
    # Different types cannot alias - this is the key TBAA insight!
    if type1 != type2:
        return False
    
    # Same type, different objects, so they could be different instances
    # but they're the same type so we have to be conservative
    return True


def optimize_load_store_tbaa(bb: Block):
    """
    Optimize loads and stores using Type-Based Alias Analysis.
    
    This is an enhanced version that understands object types and can
    eliminate more redundant operations.
    """
    opt_bb = Block()
    # Stores things we know about the heap at... compile-time.
    # Key: an object and an offset pair acting as a heap address
    # Value: a previous SSA value we know exists at that address
    compile_time_heap: Dict[Tuple[Value, int], Value] = {}
    
    for op in bb:
        if op.name == "store":
            obj = op.arg(0)
            offset = get_num(op, 1)
            store_info = (obj, offset)
            current_value = compile_time_heap.get(store_info)
            new_value = op.arg(2)
            
            # If we're storing the same value, skip the store
            if eq_value(current_value, new_value):
                continue
            
            # Invalidate all loads that may alias with this store
            # Using TBAA, we can keep loads to different object types
            compile_time_heap = {
                load_info: value
                for load_info, value in compile_time_heap.items()
                if not may_alias(load_info[0], obj, load_info[1], offset)
            }
            compile_time_heap[store_info] = new_value
            
        elif op.name == "load":
            obj = op.arg(0)
            offset = get_num(op, 1)
            load_info = (obj, offset)
            if load_info in compile_time_heap:
                op.make_equal_to(compile_time_heap[load_info])
                continue
            compile_time_heap[load_info] = op
            
        elif op.name.startswith("alloc_"):
            # Typed allocation - set the type on the operation
            if op.name in ALLOC_TYPE_MAP:
                op.type = ALLOC_TYPE_MAP[op.name]
                
        opt_bb.append(op)
    return opt_bb


# ============================================================================
# Tests from the original implementation
# ============================================================================

def test_two_loads():
    bb = Block()
    var0 = bb.getarg(0)
    var1 = bb.load(var0, 0)
    var2 = bb.load(var0, 0)
    bb.escape(var1)
    bb.escape(var2)
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = getarg(0)
var1 = load(var0, 0)
var2 = escape(var1)
var3 = escape(var1)"""
    )


def test_store_to_same_object_offset_invalidates_load():
    bb = Block()
    var0 = bb.getarg(0)
    var1 = bb.load(var0, 0)
    var2 = bb.store(var0, 0, 5)
    var3 = bb.load(var0, 0)
    bb.escape(var1)
    bb.escape(var3)
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = getarg(0)
var1 = load(var0, 0)
var2 = store(var0, 0, 5)
var3 = escape(var1)
var4 = escape(5)"""
    )


def test_store_to_same_object_different_offset_does_not_invalidate_load():
    bb = Block()
    var0 = bb.getarg(0)
    var1 = bb.load(var0, 0)
    var2 = bb.store(var0, 4, 5)
    var3 = bb.load(var0, 0)
    bb.escape(var1)
    bb.escape(var3)
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = getarg(0)
var1 = load(var0, 0)
var2 = store(var0, 4, 5)
var3 = escape(var1)
var4 = escape(var1)"""
    )


def test_load_after_store_removed():
    bb = Block()
    var0 = bb.getarg(0)
    bb.store(var0, 0, 5)
    var1 = bb.load(var0, 0)
    var2 = bb.load(var0, 1)
    bb.escape(var1)
    bb.escape(var2)
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = getarg(0)
var1 = store(var0, 0, 5)
var2 = load(var0, 1)
var3 = escape(5)
var4 = escape(var2)"""
    )


def test_store_after_store():
    bb = Block()
    arg1 = bb.getarg(0)
    bb.store(arg1, 0, 5)
    bb.store(arg1, 0, 5)
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = getarg(0)
var1 = store(var0, 0, 5)"""
    )


# ============================================================================
# New tests for TBAA functionality
# ============================================================================

def test_tbaa_different_types_no_alias():
    """
    Stores to different object types should not invalidate each other.
    This is the key improvement from TBAA!
    """
    bb = Block()
    array = bb.alloc_array()
    hash_obj = bb.alloc_hash()
    
    # Load from array
    var1 = bb.load(array, 0)
    
    # Store to hash at same offset - should NOT invalidate the array load!
    bb.store(hash_obj, 0, 42)
    
    # Load from array again - should reuse var1
    var2 = bb.load(array, 0)
    
    bb.escape(var1)
    bb.escape(var2)
    
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = alloc_array()
var1 = alloc_hash()
var2 = load(var0, 0)
var3 = store(var1, 0, 42)
var4 = escape(var2)
var5 = escape(var2)"""
    )


def test_tbaa_same_type_may_alias():
    """
    Stores to the same type but different objects should still invalidate
    loads, as they might be the same object.
    """
    bb = Block()
    array1 = bb.alloc_array()
    array2 = bb.alloc_array()
    
    # Load from array1
    var1 = bb.load(array1, 0)
    
    # Store to array2 at same offset - SHOULD invalidate because same type
    bb.store(array2, 0, 42)
    
    # Load from array1 again - cannot reuse var1
    var2 = bb.load(array1, 0)
    
    bb.escape(var1)
    bb.escape(var2)
    
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = alloc_array()
var1 = alloc_array()
var2 = load(var0, 0)
var3 = store(var1, 0, 42)
var4 = load(var0, 0)
var5 = escape(var2)
var6 = escape(var4)"""
    )


def test_tbaa_multiple_types():
    """
    Test with multiple different object types.
    """
    bb = Block()
    array = bb.alloc_array()
    hash_obj = bb.alloc_hash()
    string = bb.alloc_string()
    
    # Load from each
    arr_val = bb.load(array, 0)
    hash_val = bb.load(hash_obj, 0)
    str_val = bb.load(string, 0)
    
    # Store to hash - should only invalidate hash loads
    bb.store(hash_obj, 0, 100)
    
    # Load from each again
    arr_val2 = bb.load(array, 0)
    hash_val2 = bb.load(hash_obj, 0)
    str_val2 = bb.load(string, 0)
    
    bb.escape(arr_val2)
    bb.escape(hash_val2)
    bb.escape(str_val2)
    
    opt_bb = optimize_load_store_tbaa(bb)
    # Array and string loads should be reused, hash load should be replaced with constant
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = alloc_array()
var1 = alloc_hash()
var2 = alloc_string()
var3 = load(var0, 0)
var4 = load(var1, 0)
var5 = load(var2, 0)
var6 = store(var1, 0, 100)
var7 = escape(var3)
var8 = escape(100)
var9 = escape(var5)"""
    )


def test_tbaa_unknown_type_conservative():
    """
    Unknown types should be conservative and assume aliasing.
    """
    bb = Block()
    array = bb.alloc_array()
    unknown = bb.getarg(0)  # Unknown type
    
    # Load from array
    var1 = bb.load(array, 0)
    
    # Store to unknown object - SHOULD invalidate array load conservatively
    bb.store(unknown, 0, 42)
    
    # Load from array again - cannot reuse var1
    var2 = bb.load(array, 0)
    
    bb.escape(var1)
    bb.escape(var2)
    
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = alloc_array()
var1 = getarg(0)
var2 = load(var0, 0)
var3 = store(var1, 0, 42)
var4 = load(var0, 0)
var5 = escape(var2)
var6 = escape(var4)"""
    )


def test_tbaa_all_ruby_types():
    """
    Test all supported Ruby object types don't alias with each other.
    """
    bb = Block()
    array = bb.alloc_array()
    hash_obj = bb.alloc_hash()
    string = bb.alloc_string()
    integer = bb.alloc_integer()
    float_obj = bb.alloc_float()
    symbol = bb.alloc_symbol()
    range_obj = bb.alloc_range()
    regexp = bb.alloc_regexp()
    
    # Load from array
    arr_val = bb.load(array, 0)
    
    # Store to all other types - should NOT invalidate array load
    bb.store(hash_obj, 0, 1)
    bb.store(string, 0, 2)
    bb.store(integer, 0, 3)
    bb.store(float_obj, 0, 4)
    bb.store(symbol, 0, 5)
    bb.store(range_obj, 0, 6)
    bb.store(regexp, 0, 7)
    
    # Load from array again - should reuse arr_val
    arr_val2 = bb.load(array, 0)
    
    bb.escape(arr_val2)
    
    opt_bb = optimize_load_store_tbaa(bb)
    # The final load should be optimized away, and arr_val (var8) should be reused
    result = bb_to_str(opt_bb)
    # Check that the escape uses var8 (the original load), not a new load
    assert "escape(var8)" in result


def test_tbaa_store_load_different_types():
    """
    Store to one type, load from another - load should not be affected.
    """
    bb = Block()
    string = bb.alloc_string()
    integer = bb.alloc_integer()
    
    # Store to string
    bb.store(string, 0, "hello")
    
    # Store to integer  
    bb.store(integer, 0, 42)
    
    # Load from string - should get "hello"
    str_val = bb.load(string, 0)
    
    # Load from integer - should get 42
    int_val = bb.load(integer, 0)
    
    bb.escape(str_val)
    bb.escape(int_val)
    
    opt_bb = optimize_load_store_tbaa(bb)
    assert (
        bb_to_str(opt_bb)
        == """\
var0 = alloc_string()
var1 = alloc_integer()
var2 = store(var0, 0, hello)
var3 = store(var1, 0, 42)
var4 = escape(hello)
var5 = escape(42)"""
    )


if __name__ == "__main__":
    # Run tests
    pytest.main([__file__, "-v"])
