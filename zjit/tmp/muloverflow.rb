# frozen_string_literal: true
# Minimal reproducer: ARM64 FixnumMult spurious overflow side-exits.
# Needs: two multiplies, getbyte (for register pressure), and >>32.
# Before fix: fixnum_mult_overflow: 71, ratio_in_zjit: 3.7%
# After fix:  side_exit_count: 0,       ratio_in_zjit: 69%
def f(s)
  a = 0; b = 0; i = 0
  while i < s.bytesize
    a = a * 3 + s.getbyte(i)
    b = b * 3 + (a >> 32)
    i += 1
  end
  a
end
100.times { f("x") }
