# ARM64: Spurious FixnumMult overflow side-exits due to broken smulh/cmp pattern match

## Summary

A bug in the ARM64 backend causes `FixnumMult` to emit spurious overflow
side-exits. The JIT bails to the interpreter on nearly every function call,
reducing `ratio_in_zjit` from ~70% to ~4%. The root cause is a fragile
instruction pattern match in the emit pass that silently fails when a spill
Store is inserted between `Mul` and `RShift` by `arm64_scratch_split`.

The failure mode is **silent**: no crash, no assertion, no error. The `JoMul`
conditional branch simply reads stale CPU condition flags from a prior
instruction, and since those flags happen to say "not equal" in most cases, the
JIT side-exits to the interpreter. This makes it a **performance cliff** that
is invisible unless you check `--zjit-stats`.

In theory, if stale flags happened to indicate "equal" when a real overflow
occurred, the JIT would silently produce wrong results. We have not observed
this in practice but the possibility exists.

## Minimal reproducer

```ruby
# frozen_string_literal: true
# tmp/muloverflow.rb — two FixnumMult + getbyte + >>32 in a loop
def repro(str)
  lo = 5381
  hi = 0
  i = 0
  len = str.bytesize
  while i < len
    prod_lo = lo * 33 + str.getbyte(i)
    carry = prod_lo >> 32
    lo = prod_lo & 0xFFFFFFFF
    hi = (hi * 5 + carry) & 0xFFFFFFFF
    i += 1
  end
  lo
end

100.times { repro("hello world") }
```

**Before fix:**
```
$ ruby --zjit-stats tmp/muloverflow.rb 2>&1 | grep -E "mult_overflow|ratio_in_zjit"
  fixnum_mult_overflow: 71 (100.0%)
ratio_in_zjit:                                     3.7%
```

**After fix:**
```
$ ruby --zjit-stats tmp/muloverflow.rb 2>&1 | grep -E "mult_overflow|ratio_in_zjit"
side_exit_count:                                      0
ratio_in_zjit:                                    69.1%
```

## Conditions required to trigger

All of these must be present simultaneously:

1. **Two `FixnumMult` instructions** in the same basic block
2. **A cfunc call** (like `String#getbyte`) in between — this creates enough
   register pressure to cause the Mul output to be spilled to a stack slot
3. **A right-shift by 32** (`>> 32`) — adds more live values, increasing spill
   pressure

Removing any one of these conditions makes the bug disappear.

## Root cause

### ARM64 overflow detection for multiply

ARM64 has no overflow flag for multiplication. The standard technique is:

```asm
smulh x16, x0, x1   ; signed multiply high: upper 64 bits
mul   x0, x0, x1    ; multiply: lower 64 bits
asr   x15, x0, #63  ; sign-extend the low result
cmp   x16, x15      ; if high bits != sign-extended low, overflow
b.ne  overflow_exit
```

ZJIT implements this as a three-pass pipeline:

1. **HIR → LIR lowering** (`codegen.rs`): Emits `Mul` + `JoMul` instructions
2. **arm64_scratch_split** (`arm64/mod.rs`): Inserts `RShift` between `Mul` and
   `JoMul` to prepare the sign bit for the comparison
3. **arm64_emit** (`arm64/mod.rs`): Pattern-matches `[Mul, RShift, JoMul]` to
   fuse them into the `smulh`+`mul`+`asr`+`cmp` sequence

### The bug

When the Mul output register is spilled (allocated to a stack slot by
`alloc_regs`), `arm64_scratch_split` also inserts a `Store` instruction to
write the result to the stack. The code inserts the Store **before** the
RShift, producing:

```
Mul x15, x15, x17
Store [x29 - 8], x15     ← spill
RShift x15, x15, 63      ← sign bit extraction
JoMul side_exit
```

The emit pass checks `insns[idx+1]` and `insns[idx+2]` for `RShift` and
`JoMul`:

```rust
match (insns.get(insn_idx + 1), insns.get(insn_idx + 2)) {
    (Some(Insn::RShift { .. }), Some(Insn::JoMul(_))) => {
        // emit smulh + mul + asr + cmp
    }
    _ => {
        mul(cb, out, left, right);  // ← NO smulh, NO cmp
    }
}
```

With the Store in between, `insns[idx+1]` is `Store`, not `RShift`. The
pattern match **falls through to the else branch**, which emits only `mul`
without `smulh` or `cmp`. The `RShift` is then emitted as a standalone `asr`,
and `JoMul` emits `b.ne` — but `cmp` was never executed, so `b.ne` reads
**stale condition flags** from whatever instruction last set them.

### Consequence

- **If stale flags = NE (common):** Spurious side-exit. The function drops to
  interpreter speed. ~20x perf regression on affected code.
- **If stale flags = EQ during real overflow (rare):** Missed overflow. The
  mul result wraps without promoting to Bignum. **Silent wrong result.**

## Fix

The fix has two parts:

### 1. `arm64_scratch_split`: Reorder Store to after RShift when JoMul follows

When the next instruction is `JoMul`, emit the `RShift` immediately after
`Mul`, and move the spill `Store` to after the `RShift`. The Store now writes
from the Mul output register directly (rather than SCRATCH0, which was
clobbered by the RShift):

```
Mul x15, x15, x17        ← mul result in x15
RShift x15, x15, 63      ← sign bit (clobbers x15)
Store [x29 - 8], x15     ← stores sign bit, NOT mul result
JoMul side_exit
```

Wait — this stores the sign bit, not the mul result! So the emit pass must
handle this.

### 2. `arm64_emit`: Handle `[Mul, RShift, Store, JoMul]` pattern

The emit pass now recognizes both patterns:

- `[Mul, RShift, JoMul]` — original (no spill)
- `[Mul, RShift, Store, JoMul]` — with spill

For the spill case, the emitted ARM64 code is:

```asm
smulh x16, x0, x1    ; high 64 bits
mul   x15, x0, x1    ; low 64 bits
stur  x15, [x29, #-8] ; spill mul result BEFORE asr clobbers x15
asr   x15, x15, #63  ; sign bit of low result
cmp   x16, x15       ; overflow check
b.ne  overflow_exit
```

The `stur` is emitted between `mul` and `asr`, preserving the mul result in
the spill slot before `asr` clobbers the register. The `smulh` result in X16
is safe because `stur` with a register source doesn't touch X16.

## Bisection table

These tests were run on the pre-fix build to isolate the trigger conditions:

| Variant | Overflows? | ratio_in_zjit |
|---------|-----------|---------------|
| `lo * 33`, no hi, no getbyte | No | 69% |
| `lo * 33 + getbyte`, no hi | No | 68% |
| `lo * 33 + getbyte`, `>> 32`, no hi multiply | No | 69% |
| `lo * 33 + getbyte`, `>> 32`, `hi * 33 + carry` (full) | **Yes** | 3.7% |
| Same but `carry` computed but unused | **Yes** | 3.7% |
| Same but `(lo << 5) + lo` instead of `lo * 33` | No | 68% |
| `lo * 33 + (i & 0xFF)` instead of getbyte, with hi | No | 69% |
| `hi * 33 + constant` in isolation | No | 69% |

## Lessons and recommendations

### 1. Avoid fragile peephole pattern matching across passes

The core issue is that `arm64_scratch_split` and `arm64_emit` communicate via
an **implicit contract**: "RShift will be at idx+1 after Mul". When
`scratch_split` inserts a Store, this contract is silently violated. Neither
pass checks or asserts the invariant.

**Recommendation**: Use an explicit fused instruction (`MulWithOverflowCheck`)
in the LIR instead of relying on Mul+RShift+JoMul being contiguous. This is
what other compilers do:

- **LLVM**: Uses `llvm.smul.with.overflow` intrinsic — a single instruction
  that produces both the result and an overflow bit.
- **Cranelift**: Uses `imul` + `trapif` where the overflow flag is an explicit
  operand, not a side effect.
- **V8 TurboFan**: Uses `Int32MulWithOverflow` as a single node that lowers to
  the platform-specific sequence atomically.
- **GCC**: The `-ftrapv` multiply overflow check is emitted as a single
  inseparable sequence during final code emission, never as separate
  matchable instructions.

### 2. Reject unknown instruction sequences

When the emit pass encounters `Mul` without the expected `RShift + JoMul`
pattern, it falls through to a plain `mul`. But if there IS a `JoMul` later
(just not at idx+2), the `JoMul` will execute with wrong flags. The else
branch should either:

- **Panic**: "Mul followed by JoMul but RShift not at expected position"
- **Emit a safe fallback**: `smulh` + `mul` + `asr` + `cmp` unconditionally

The current silent fallthrough is the worst option.

### 3. Add end-to-end overflow tests with register pressure

The existing tests only exercise `FixnumMult` in simple functions with low
register pressure (where the output isn't spilled). A test with two multiplies
and a cfunc call would have caught this immediately.

### 4. Consider separating "instruction lowering" from "instruction selection"

The ARM64 backend conflates two concerns:

- **Lowering**: Converting abstract LIR to concrete register operations,
  handling spills (scratch_split)
- **Selection**: Fusing multiple LIR instructions into a single ARM64 sequence
  (emit)

Other compilers (LLVM, Cranelift) keep these clearly separated, with selection
happening before register allocation. Post-regalloc passes only handle
mechanical concerns (encoding, addressing modes) and never need to pattern
match across multiple instructions.

## Files changed

- `zjit/src/backend/arm64/mod.rs` — scratch_split Mul handling + emit Mul
  pattern match

## Test results

- 1454 unit tests: all pass
- 27 integration tests, 7605 assertions: all pass
- Reproducer: 0 side exits, 69.1% ratio_in_zjit (was 71 exits, 3.7%)
- DJB2 64-bit hash: correct results match interpreter
