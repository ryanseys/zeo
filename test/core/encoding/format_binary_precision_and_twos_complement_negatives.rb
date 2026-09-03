# %b honors precision (min digits, disabling the 0-flag) and renders
# negatives in CRuby's infinite two's-complement ".." notation, switching
# to signed magnitude under a sign flag.

p("%08b" % 10)
p("%.8b" % 5)
p("%#010b" % 10)
p("%05.3b" % 0)
p("%b" % -5)
p("%.8b" % -5)
p("%010b" % -5)
p("%#010b" % -5)
p("%+b" % -5)
p("%+08b" % -5)
__END__
"00001010"
"00000101"
"0b00001010"
"  000"
"..1011"
"..111011"
"..11111011"
"0b..111011"
"-101"
"-0000101"
