# From the spinel corpus (fa06b601).
#
# `/i` folds only ASCII, so a non-ASCII literal does not match its
# counterpart.
#
# CRuby folds by Unicode simple case folding, and a counterpart need not have
# the same WIDTH (`U+212A` folds to `k`), which is why the fix is to emit the
# counterparts as a class rather than to fold in place.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# /i folds by Unicode simple case folding: the counterparts of a non-ASCII
# literal are emitted as a class, since a counterpart need not have the same
# width (U+212A folds to `k`). Ported from mruby-regexp (618ba9435). A build
# made with -DRE_NO_UNICODE_CASE folds ASCII alone and matches the rest
# literally, which is what this engine did before the table arrived.
p("Ä" =~ /ä/i ? true : false)
p("ä" =~ /Ä/i ? true : false)
p("Ä" =~ /ä/ ? true : false)
p("Σ" =~ /σ/i ? true : false)
p("ς" =~ /σ/i ? true : false)
p("K" =~ /k/i ? true : false)
p("ÄB" =~ /äb/i ? true : false)
p "ÄäÖ".scan(/ä/i)
p "ÄäÖ".gsub(/ä/i, "-")

# character classes fold their codepoint members and ranges
p("ä" =~ /[Ä]/i ? true : false)
p("ä" =~ /[À-Þ]/i ? true : false)
p("Ä" =~ /[^ä]/i ? true : false)
p("a" =~ /[A-Z]/i ? true : false)
p("あ" =~ /[あ-ん]/i ? true : false)

# and a backreference compares folded codepoints, whatever their widths
p("Ää" =~ /(Ä)\1/i ? true : false)
p("Kk" =~ /(K)\1/i ? true : false)
p("ÄÖ" =~ /(Ä)\1/i ? true : false)

# A source whose fold is several codepoints (U+00DF to "ss") has no single
# counterpart to fold to and is matched literally, so `"ß" =~ /ss/i` answers
# nil where CRuby answers 0 -- the same scope mruby-regexp draws.
p("ß" =~ /ß/i ? true : false)
__END__
true
true
false
true
true
true
true
["Ä", "ä"]
"--Ö"
true
true
false
true
true
true
true
false
true
