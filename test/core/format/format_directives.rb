# `format` (aka `sprintf`, and `String#%`) supports the full C-style directive
# set, plus Ruby's named and positional references.

# Named references pull from a Hash / keyword arguments.
puts format("%<name>s is %<age>d", name: "Ada", age: 36)   # Ada is 36
puts format("%{greeting}, world!", greeting: "Hello")      # Hello, world!

# Binary (%b / %B) and the `#` alternate form add a radix prefix.
puts format("%b", 10)                                      # 1010
puts format("%#b", 10)                                     # 0b1010
puts format("%#x / %#o", 255, 8)                           # 0xff / 010
puts format("%08b", 10)                                    # 00001010

# `*` reads the width (or precision) from an argument; a negative width
# left-justifies.
puts format("%*d|", 5, 42)                                 #    42|
puts format("%-*d|", 5, 42)                                # 42   |
puts format("%.*f", 2, 3.14159)                            # 3.14

# Positional (`N$`) references reuse and reorder arguments.
puts format("%2$s %1$s", "world", "hello")                 # hello world

# Uppercase variants and hex float.
puts format("%X", 255)                                     # FF
puts format("%a", 0.5)                                     # 0x1p-1

# All of this works through String#% too.
puts("%<x>05.2f" % { x: 3.14159 })                         # 03.14
__END__
Ada is 36
Hello, world!
1010
0b1010
0xff / 010
00001010
   42|
42   |
3.14
hello world
FF
0x1p-1
03.14
