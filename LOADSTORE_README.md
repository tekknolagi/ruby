# Load/Store Optimization with Type-Based Alias Analysis (TBAA)

This implementation demonstrates a load/store optimization pass enhanced with Type-Based Alias Analysis (TBAA). It builds upon the basic load/store optimization from https://bernsteinbear.com/blog/toy-load-store/ by adding type information to distinguish memory operations on different object types.

## Overview

The optimizer performs redundant load elimination and dead store elimination by tracking what values are stored at which memory locations during compilation. The key enhancement is that it uses type information to determine when two memory operations can or cannot alias.

## Key Concepts

### Basic Load/Store Optimization

The baseline optimization (from the blog post) tracks memory operations and can:
- Eliminate redundant loads from the same location
- Forward stored values to subsequent loads
- Remove stores that are immediately overwritten
- Remove stores whose value we just loaded

However, it must be conservative when dealing with unknown aliasing - if we store to object B, we must invalidate loads from object A at the same offset, because A and B might be the same object.

### Type-Based Alias Analysis (TBAA)

TBAA adds a crucial insight: **memory operations on different object types cannot alias**. 

For example:
- A store to an Array object cannot affect a load from a Hash object
- Even if they're at the same offset, they're guaranteed to be different memory locations
- This allows the optimizer to keep more loads valid across stores

## Implementation Details

### Object Types

The implementation defines these Ruby object types for TBAA:

```python
class ObjectType(Enum):
    UNKNOWN = auto()  # Unknown or generic object  
    ARRAY = auto()    # Ruby Array
    HASH = auto()     # Ruby Hash
    STRING = auto()   # Ruby String
    INTEGER = auto()  # Ruby Integer (Fixnum/Bignum)
    FLOAT = auto()    # Ruby Float
    SYMBOL = auto()   # Ruby Symbol
    RANGE = auto()    # Ruby Range
    REGEXP = auto()   # Ruby Regexp
```

### Aliasing Rules

The `may_alias()` function implements these rules:

1. **Same object, same offset**: Always aliases
2. **Same object, different offset**: Never aliases (different fields)
3. **Different objects, different types**: Never aliases (TBAA!)
4. **Different objects, same type**: May alias (conservative - they could be the same)
5. **Unknown type involved**: May alias (conservative)

### Typed Allocation Operations

New operations for allocating typed objects:
- `alloc_array()` - Allocate an Array (type = ARRAY)
- `alloc_hash()` - Allocate a Hash (type = HASH)
- `alloc_string()` - Allocate a String (type = STRING)
- etc.

These operations automatically set the type information on the created object.

## Examples

### Example 1: Different Types Don't Alias

```python
bb = Block()
array = bb.alloc_array()
hash_obj = bb.alloc_hash()

var1 = bb.load(array, 0)
bb.store(hash_obj, 0, 42)  # Store to hash
var2 = bb.load(array, 0)   # Can reuse var1!
```

**Before optimization:**
```
var0 = alloc_array()
var1 = alloc_hash()
var2 = load(var0, 0)
var3 = store(var1, 0, 42)
var4 = load(var0, 0)
```

**After TBAA optimization:**
```
var0 = alloc_array()
var1 = alloc_hash()
var2 = load(var0, 0)
var3 = store(var1, 0, 42)
# var4 eliminated! Reuses var2
```

### Example 2: Same Type, Different Objects

```python
bb = Block()
array1 = bb.alloc_array()
array2 = bb.alloc_array()

var1 = bb.load(array1, 0)
bb.store(array2, 0, 42)    # Store to different array
var2 = bb.load(array1, 0)  # Cannot reuse var1 (conservative)
```

Because both are Arrays, we cannot prove they're different objects, so we must be conservative.

### Example 3: Multiple Types

```python
bb = Block()
array = bb.alloc_array()
hash_obj = bb.alloc_hash()
string = bb.alloc_string()

arr_val = bb.load(array, 0)
hash_val = bb.load(hash_obj, 0)
str_val = bb.load(string, 0)

bb.store(hash_obj, 0, 100)  # Only invalidates hash loads!

# These reuse the original loads:
arr_val2 = bb.load(array, 0)   # Reuses arr_val
str_val2 = bb.load(string, 0)  # Reuses str_val

# This gets the stored value:
hash_val2 = bb.load(hash_obj, 0)  # Replaced with constant 100
```

## Benefits

1. **More aggressive optimization**: Can eliminate more redundant loads
2. **Better performance**: Fewer memory operations in the generated code
3. **Type safety**: Leverages the type system for correctness
4. **Scalability**: Works well with many object types

## Limitations

1. **Conservative for unknown types**: Must assume aliasing when type is unknown
2. **Same type aliasing**: Cannot optimize across same-type different-object stores
3. **No flow-sensitive analysis**: Uses a single pass, doesn't track control flow
4. **No escape analysis**: Doesn't distinguish between local and escaped objects

## Testing

Run the test suite with:

```bash
python3 -m pytest loadstore.py -v
```

The tests cover:
- Basic load/store optimization scenarios
- TBAA with different object types
- Conservative behavior for unknown types
- All supported Ruby object types
- Edge cases with aliasing

## References

- Original blog post: https://bernsteinbear.com/blog/toy-load-store/
- TBAA in LLVM: https://llvm.org/docs/LangRef.html#tbaa-metadata
- Type-based alias analysis: Classic compiler optimization technique
