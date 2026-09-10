# String#oct detects its base from the prefix: 0x is hex, 0b binary, 0o
# explicit octal, a leading 0 implicit octal, and bare digits base 8. So
# "0xff".oct is 255, not 0.

p "0xff".oct      # hex prefix: 255
p "0b101".oct     # binary prefix: 5
p "0o77".oct      # explicit octal: 63
p "77".oct        # bare digits base-8: 63
p "010".oct       # implicit octal: 8
__END__
255
5
63
63
8
