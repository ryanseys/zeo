# GAP -- imported from the spinel corpus at fa06b601.
#
# A backreference compares case-SENSITIVELY even when the pattern is folded,
# so `/(a)\1/i` does not match `"aA"` where CRuby's does.
#
# The comparison is a plain byte compare; under `/i` it has to fold both
# sides the way the rest of the pattern already does.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# The backreference comparison was a plain memcmp, so it stayed case-sensitive
# even when the rest of the pattern was folded. Ported from mruby-regexp
# (f9adb3017); folding stops at ASCII, as it does everywhere else here.
p("aA" =~ /(a)\1/i ? true : false)
p("aA" =~ /(a)\1/ ? true : false)
p("abAB" =~ /(ab)\1/i ? true : false)
p("aa" =~ /(a)\1/ ? true : false)
p("aA" =~ /(a)\k<1>/i ? true : false)
p("aA".match(/(?<g>a)\k<g>/i)[0])
p("aA".match(/(?'g'a)\k'g'/i)[0])
p "xAxa".scan(/(a)\1/i).size
p("ÄÄ" =~ /(Ä)\1/i ? true : false)
__END__
true
false
true
true
true
"aA"
"aA"
0
true
