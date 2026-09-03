p 0b1010.allbits?(0b0010)
p 0b1010.allbits?(0b0110)
p 0b1010.anybits?(0b0110)
p 0b1010.nobits?(0b0101)
s = "hello"; s.bytesplice(0, 2, "XY"); p s
t = "hello"; t.bytesplice(1..2, "__"); p t
__END__
true
false
true
true
"XYllo"
"h__lo"
