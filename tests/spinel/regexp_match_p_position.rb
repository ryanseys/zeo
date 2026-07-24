# Issue #869: Regexp#match?(str, pos) starts matching at byte
# offset `pos`. Previously the position argument was silently
# dropped and the call returned true for any non-empty pattern.
puts /world/.match?("hello world", 6)
puts /hello/.match?("hello world", 6)
puts /hello/.match?("hello world")
puts /world/.match?("hello world", -5)
