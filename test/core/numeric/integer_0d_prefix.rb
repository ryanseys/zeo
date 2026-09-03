# GAP -- imported from the spinel corpus at c55d9bdb.
# Integer() rejects the 0d decimal prefix.
#
# `0d19` is CRuby's explicit decimal prefix; the parser knew 0x, 0b and 0o but
# not this one, and rejected the string.
r001 = (Integer("0d19") rescue $!.class)
p r001
r002 = (Integer("0D19") rescue $!.class); p r002
r003 = (Integer("0d19", 10) rescue $!.class); p r003
p(Integer("0x1f"))
p(Integer("0b101"))
p(Integer("0o17"))
p(Integer("017"))
p("0d19".to_i(10))
p(Integer("0d0"))
p(Integer(" 0d19 "))
__END__
19
19
19
31
5
15
15
19
0
19
