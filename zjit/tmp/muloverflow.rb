# frozen_string_literal: true
# Minimal no-loop reproducer for ARM64 FixnumMult spurious overflow.
# 7 getbyte calls exhaust registers, forcing Mul output to spill.
# Before fix: fixnum_mult_overflow: 71, ratio_in_zjit: 35%
# After fix:  side_exit_count: 0,       ratio_in_zjit: 69%
def f(s)
  v0 = s.getbyte(0); v1 = s.getbyte(1); v2 = s.getbyte(2)
  v3 = s.getbyte(3); v4 = s.getbyte(4); v5 = s.getbyte(5)
  v6 = s.getbyte(6)
  a = v0 * 3 + v1
  b = a * 3 + (a >> 32)
  a + b + v2 + v3 + v4 + v5 + v6
end
100.times { f("hello!!") }
