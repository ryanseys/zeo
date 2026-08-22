# GAP -- imported from the spinel corpus at fa06b601.
#
# A multibyte literal in a pattern is emitted one BYTE at a time, so each
# byte becomes an atom of its own and a quantifier binds to the last one:
# `/あ+/` compiles as the lead bytes followed by a repeat of the final byte.
#
# The visible result is a match that returns bytes which are not a character:
# `"ĀĀ"` where CRuby answers `"あ"`.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# A literal is emitted one byte at a time, so a character above 127 made each
# of its bytes an atom of its own and a quantifier bound to the last one:
# /あ+/ compiled as the lead byte followed by a repeat of the final
# continuation byte. Ported from mruby-regexp (bcacba1e1).
p("ああ" =~ /あ+/ ? $~[0] : nil)
p("ああ" =~ /あ{2}/ ? true : false)
p "ああxあ".scan(/あ+/)
p "ĀĀ".match(/Ā+/)[0]
p "ĀĀ".match?(/Ā{2}/)
p("あか" =~ /あ|か/ ? $~[0] : nil)
p("aあb" =~ /a.b/ ? true : false)
p "aあbあc".gsub(/あ/, "-")
p("ああ" =~ /\あ+/ ? $~[0] : nil)
p "あいう".index(/い/)
