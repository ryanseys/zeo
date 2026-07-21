# String#oct with prefix auto-detection.
# CRuby's oct handles 0x (hex), 0b (binary), 0o (explicit octal),
# 0 (implicit octal), and bare digits (base 8). Previously spinel
# used strtoll(base=8) unconditionally, so "0xff".oct returned 0
# instead of 255.

p "0xff".oct      # hex prefix: 255
p "0b101".oct     # binary prefix: 5
p "0o77".oct      # explicit octal: 63
p "77".oct        # bare digits base-8: 63
p "010".oct       # implicit octal: 8
