# Issue #882: `s << int` appends the UTF-8 character with that
# codepoint (per MRI). Previously appended the integer's decimal
# digits.
s = String.new("hello")
s << 33
puts s.inspect
s2 = String.new("ab")
s2 << 12354
puts s2.inspect
