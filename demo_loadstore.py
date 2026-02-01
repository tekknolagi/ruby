#!/usr/bin/env python3
"""
Demonstration of TBAA-based load/store optimization.

This script shows concrete examples of how Type-Based Alias Analysis
improves load/store optimization by understanding object types.
"""

from loadstore import (
    Block, bb_to_str, optimize_load_store_tbaa
)


def demo_basic_optimization():
    """Show basic load elimination without TBAA."""
    print("=" * 70)
    print("DEMO 1: Basic Load Elimination")
    print("=" * 70)
    print("\nScenario: Two loads from the same location\n")
    
    bb = Block()
    obj = bb.getarg(0)
    val1 = bb.load(obj, 0)
    val2 = bb.load(obj, 0)
    bb.escape(val1)
    bb.escape(val2)
    
    print("Before optimization:")
    print(bb_to_str(bb))
    print()
    
    opt_bb = optimize_load_store_tbaa(bb)
    print("After optimization:")
    print(bb_to_str(opt_bb))
    print("\n✓ Second load eliminated - reuses first load\n")


def demo_tbaa_different_types():
    """Show how TBAA enables optimization across different types."""
    print("=" * 70)
    print("DEMO 2: TBAA - Different Types Don't Alias")
    print("=" * 70)
    print("\nScenario: Load from Array, store to Hash, load from Array again\n")
    
    bb = Block()
    array = bb.alloc_array()
    hash_obj = bb.alloc_hash()
    
    # Load from array
    arr_val1 = bb.load(array, 0)
    
    # Store to hash (different type!)
    bb.store(hash_obj, 0, 42)
    
    # Load from array again
    arr_val2 = bb.load(array, 0)
    
    bb.escape(arr_val1)
    bb.escape(arr_val2)
    
    print("Before optimization:")
    print(bb_to_str(bb))
    print()
    
    opt_bb = optimize_load_store_tbaa(bb)
    print("After optimization:")
    print(bb_to_str(opt_bb))
    print("\n✓ Array load not invalidated by Hash store - different types!")
    print("✓ Second array load eliminated\n")


def demo_tbaa_same_type():
    """Show conservative behavior for same type."""
    print("=" * 70)
    print("DEMO 3: TBAA - Same Type, Conservative Behavior")
    print("=" * 70)
    print("\nScenario: Load from Array1, store to Array2, load from Array1 again\n")
    
    bb = Block()
    array1 = bb.alloc_array()
    array2 = bb.alloc_array()
    
    # Load from array1
    arr_val1 = bb.load(array1, 0)
    
    # Store to array2 (same type, different object)
    bb.store(array2, 0, 42)
    
    # Load from array1 again
    arr_val2 = bb.load(array1, 0)
    
    bb.escape(arr_val1)
    bb.escape(arr_val2)
    
    print("Before optimization:")
    print(bb_to_str(bb))
    print()
    
    opt_bb = optimize_load_store_tbaa(bb)
    print("After optimization:")
    print(bb_to_str(opt_bb))
    print("\n✗ Array load IS invalidated - they might be the same object")
    print("✗ Second array load cannot be eliminated (conservative)\n")


def demo_multiple_types():
    """Show TBAA working with multiple object types."""
    print("=" * 70)
    print("DEMO 4: TBAA - Multiple Object Types")
    print("=" * 70)
    print("\nScenario: Multiple stores to different types, loads from original\n")
    
    bb = Block()
    array = bb.alloc_array()
    hash_obj = bb.alloc_hash()
    string = bb.alloc_string()
    symbol = bb.alloc_symbol()
    
    # Load from array
    arr_val1 = bb.load(array, 0)
    
    # Store to multiple different types at same offset
    bb.store(hash_obj, 0, 1)
    bb.store(string, 0, 2)
    bb.store(symbol, 0, 3)
    
    # Load from array again
    arr_val2 = bb.load(array, 0)
    
    bb.escape(arr_val2)
    
    print("Before optimization:")
    print(bb_to_str(bb))
    print()
    
    opt_bb = optimize_load_store_tbaa(bb)
    print("After optimization:")
    print(bb_to_str(opt_bb))
    print("\n✓ Array load survives all stores to different types!")
    print("✓ Final array load eliminated\n")


def demo_store_forwarding():
    """Show store-to-load forwarding with TBAA."""
    print("=" * 70)
    print("DEMO 5: Store-to-Load Forwarding with TBAA")
    print("=" * 70)
    print("\nScenario: Store then load from different typed objects\n")
    
    bb = Block()
    string = bb.alloc_string()
    integer = bb.alloc_integer()
    
    # Store to string
    bb.store(string, 0, "hello")
    
    # Store to integer
    bb.store(integer, 0, 42)
    
    # Load from both
    str_val = bb.load(string, 0)
    int_val = bb.load(integer, 0)
    
    bb.escape(str_val)
    bb.escape(int_val)
    
    print("Before optimization:")
    print(bb_to_str(bb))
    print()
    
    opt_bb = optimize_load_store_tbaa(bb)
    print("After optimization:")
    print(bb_to_str(opt_bb))
    print("\n✓ Both loads replaced with stored constants")
    print("✓ Stores to different types don't interfere\n")


def demo_offset_independence():
    """Show how different offsets on same object don't alias."""
    print("=" * 70)
    print("DEMO 6: Different Offsets Don't Alias")
    print("=" * 70)
    print("\nScenario: Load offset 0, store offset 4, load offset 0 again\n")
    
    bb = Block()
    obj = bb.alloc_hash()
    
    # Load from offset 0
    val1 = bb.load(obj, 0)
    
    # Store to offset 4 (different field!)
    bb.store(obj, 4, 99)
    
    # Load from offset 0 again
    val2 = bb.load(obj, 0)
    
    bb.escape(val1)
    bb.escape(val2)
    
    print("Before optimization:")
    print(bb_to_str(bb))
    print()
    
    opt_bb = optimize_load_store_tbaa(bb)
    print("After optimization:")
    print(bb_to_str(opt_bb))
    print("\n✓ Load at offset 0 not affected by store at offset 4")
    print("✓ Second load eliminated\n")


def main():
    print("\n" + "=" * 70)
    print(" TBAA Load/Store Optimization Demonstration")
    print("=" * 70 + "\n")
    
    demo_basic_optimization()
    demo_tbaa_different_types()
    demo_tbaa_same_type()
    demo_multiple_types()
    demo_store_forwarding()
    demo_offset_independence()
    
    print("=" * 70)
    print(" Summary")
    print("=" * 70)
    print("""
Type-Based Alias Analysis (TBAA) enables more aggressive optimizations by:

1. ✓ Distinguishing between different object types (Array vs Hash)
2. ✓ Allowing loads to survive stores to different types
3. ✓ Reducing redundant memory operations
4. ✓ Maintaining correctness with conservative fallbacks

This leads to:
- Fewer memory operations in generated code
- Better performance
- Leveraging type system for optimization

Run the tests with: python3 -m pytest loadstore.py -v
""")


if __name__ == "__main__":
    main()
