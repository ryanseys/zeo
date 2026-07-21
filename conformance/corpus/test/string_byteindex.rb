# String#byteindex/#byterindex return BYTE offsets (not codepoint indices),
# with byte-offset start/pos arguments; nil on miss.
p "abcabc".byteindex("b")
p "abcabc".byteindex("b", 2)
p "café".byteindex("f")
p "café".byteindex("é")
p "hello".byteindex("z")
p "hello".byteindex("")
p "abcabc".byterindex("b")
p "abcabc".byterindex("b", 3)
p "café".byteindex("f", -2)
p "aaa".byteindex("a", 1)
