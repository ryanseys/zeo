# String#append_as_bytes accepts Integer byte values (100 -> "d") as well as
# Strings, and a mix of both.
a = "abc"; a.append_as_bytes(100, 101); p a
b = "x"; b.append_as_bytes("y", 122); p b
c = "".dup; c.append_as_bytes(72, 73); p c
