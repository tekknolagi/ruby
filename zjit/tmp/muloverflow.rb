# frozen_string_literal: true
# Minimal: two mults + getbyte + >> 32
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
