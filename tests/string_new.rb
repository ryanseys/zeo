# `String.new` builds a fresh, mutable String. With no argument it is an empty
# ASCII-8BIT buffer; given a source string it copies that string's contents and
# encoding, and an `encoding:` keyword overrides the result's encoding.
p String.new                              # ""
p String.new.encoding.name                # "ASCII-8BIT"

p String.new("hello")                     # "hello"
p String.new("hello").encoding.name       # "UTF-8"

forced = String.new("bytes", encoding: "ASCII-8BIT")
p forced.encoding.name                    # "ASCII-8BIT"

# The copy is independent of its source.
src = "abc"
copy = String.new(src)
copy << "d"
p src                                     # "abc"
p copy                                    # "abcd"
