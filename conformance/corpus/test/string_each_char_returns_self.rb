# Issue #866: String#each_char with a block returns the receiver
# (not nil and not 0).
s = "hello".each_char {}
puts s

# Inside the block, characters are yielded.
out = []
"abc".each_char { |c| out.push(c) }
puts out.inspect

# Multi-byte UTF-8 chars are yielded as whole code points.
mb_chars = []
"héllo".each_char { |c| mb_chars.push(c) }
puts mb_chars.length
puts mb_chars.inspect
