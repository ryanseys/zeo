# Array#grep(pattern) / #grep_v(pattern) without a block: collect the
# elements matching (or not matching) the pattern via `pattern ===`.
# Supported: Range over an int array, primitive Class over a poly array,
# Regexp over a str array. (The block form falls through to unresolved.)
p [1, 2, 3, 4, 5].grep(1..3)
p [1, 2, 3, 4, 5].grep_v(1..3)
p [1, 2, 3, 4, 5].grep(2...4)
p [1, 2].grep(10..20)
p [1, "two", 3, "four"].grep(String)
p [1, "two", 3, "four"].grep(Integer)
p ["apple", "banana", "avocado"].grep(/^a/)
p ["apple", "banana", "avocado"].grep_v(/^a/)
