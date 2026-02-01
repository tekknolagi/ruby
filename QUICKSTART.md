# TBAA Load/Store Optimization - Quick Start

This directory contains a standalone implementation of Type-Based Alias Analysis (TBAA) for load/store optimization in a toy IR.

## Quick Start

### 1. Run the Tests
```bash
python3 -m pytest loadstore.py -v
```

Expected output: `11 passed`

### 2. Run the Interactive Demo
```bash
python3 demo_loadstore.py
```

This shows 6 concrete examples of how TBAA improves optimization.

### 3. Try It Yourself

```python
from loadstore import Block, bb_to_str, optimize_load_store_tbaa

# Create a basic block
bb = Block()

# Allocate typed objects
array = bb.alloc_array()
hash_obj = bb.alloc_hash()

# Load from array
val1 = bb.load(array, 0)

# Store to hash (different type!)
bb.store(hash_obj, 0, 42)

# Load from array again
val2 = bb.load(array, 0)

bb.escape(val1)
bb.escape(val2)

# Optimize
opt_bb = optimize_load_store_tbaa(bb)

# Show results
print("Before:")
print(bb_to_str(bb))
print("\nAfter:")
print(bb_to_str(opt_bb))
```

## What's Different from the Original?

The base implementation from https://bernsteinbear.com/blog/toy-load-store/ is conservative: any store could potentially invalidate any load at the same offset.

**This TBAA implementation adds:**
- Object type tracking (Array, Hash, String, etc.)
- Type-aware aliasing rules
- Ability to prove non-aliasing between different types
- More aggressive optimization while remaining correct

## Key Files

- **loadstore.py** - Main implementation (360 lines)
- **demo_loadstore.py** - Interactive demonstration (200 lines)
- **LOADSTORE_README.md** - Detailed documentation

## Supported Object Types

```python
ObjectType.ARRAY    # Ruby Array
ObjectType.HASH     # Ruby Hash  
ObjectType.STRING   # Ruby String
ObjectType.INTEGER  # Ruby Integer
ObjectType.FLOAT    # Ruby Float
ObjectType.SYMBOL   # Ruby Symbol
ObjectType.RANGE    # Ruby Range
ObjectType.REGEXP   # Ruby Regexp
ObjectType.UNKNOWN  # Unknown/generic
```

## Typed Allocation Operations

```python
array = bb.alloc_array()      # Creates an Array
hash = bb.alloc_hash()         # Creates a Hash
string = bb.alloc_string()     # Creates a String
integer = bb.alloc_integer()   # Creates an Integer
float_obj = bb.alloc_float()   # Creates a Float
symbol = bb.alloc_symbol()     # Creates a Symbol
range_obj = bb.alloc_range()   # Creates a Range
regexp = bb.alloc_regexp()     # Creates a Regexp
```

## Aliasing Rules

1. **Same object, same offset**: Always aliases ✓
2. **Same object, different offset**: Never aliases ✓
3. **Different objects, different types**: Never aliases ✓ (TBAA!)
4. **Different objects, same type**: May alias ⚠️ (conservative)
5. **Unknown type involved**: May alias ⚠️ (conservative)

## Performance Impact

The TBAA optimization enables:
- Elimination of redundant loads across type boundaries
- Fewer memory operations in generated code
- Better cache utilization
- Improved performance in type-stable code

## Next Steps

1. Read LOADSTORE_README.md for detailed documentation
2. Run the tests and demos
3. Experiment with your own examples
4. Consider integrating into a real compiler!

## References

- Original blog post: https://bernsteinbear.com/blog/toy-load-store/
- Reference implementation: https://github.com/tekknolagi/tekknolagi.github.com/blob/main/loadstore.py
- TBAA in LLVM: https://llvm.org/docs/LangRef.html#tbaa-metadata
