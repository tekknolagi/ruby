str = 'abc€›ﬁ!‰,' * 1_000_000
1_000.times do
  str.force_encoding(Encoding::UTF_8) # clear coderange
  str.valid_encoding?
end
