# A grab-bag of String / Hash leaf methods.

# `gsub` with a Hash maps each match to its replacement (missing keys -> "").
puts "hello world".gsub(/[aeiou]/, "a" => "1", "e" => "2", "o" => "3")   # h2ll1 w3rld

# `split` with an empty separator yields characters; a single space splits on
# whitespace runs; a limit caps the field count.
p "hello".split("")                    # ["h", "e", "l", "l", "o"]
p "a b  c   d".split(" ")              # ["a", "b", "c", "d"]
p "a,b,c,d".split(",", 2)              # ["a", "b,c,d"]

# Regexp indexing pulls out the whole match or a capture group.
p "hello world foo"[/\w+/]             # "hello"
p "hello world"[/(\w+) (\w+)/, 2]      # "world"

# Case-insensitive comparison and radix parsing.
p "Hello".casecmp("hello")             # 0
p "Hello".casecmp?("HELLO")            # true
p "0x1f".oct                           # 31
p "777".oct                            # 511
p "ff".hex                             # 255

# `slice!` removes a span in place and returns it.
s = "hello"
p s.slice!(1, 2)                       # "el"
p s                                    # "hlo"

# `Hash#sort` orders the [key, value] pairs.
p({ b: 2, a: 1, c: 3 }.sort)           # [[:a, 1], [:b, 2], [:c, 3]]

# Numeric stepping, integer and float lanes.
p 1.step(10, 3).to_a                   # [1, 4, 7, 10]
p 1.0.step(2.0, 0.5).to_a              # [1.0, 1.5, 2.0]
