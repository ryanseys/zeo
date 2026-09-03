# Ruby's case-mapping OPTIONS -- `:ascii`, `:turkic`, `:lithuanian`, `:fold`
# -- and the acceptance rules around them, which follow from nothing but
# CRuby's `check_case_options`: `:turkic` and `:lithuanian` may be given
# together in either order, nothing else may be given twice, `:fold` is
# downcasing only, and each refusal has its own message.
#
# They are not decoration. `"straße".upcase(:ascii)` is `"STRAßE"` where the
# full mapping gives `"STRASSE"` and the string gets LONGER -- a protocol
# that wants ASCII casing and gets Unicode's has silently changed its data.
#
# `:fold` is case FOLDING, which is not lowercasing: `ß` folds to `"ss"` and
# `ﬁ` to `"fi"`. The 297 codepoints where the two differ are generated from
# the oracle (`tools/casefold_table.rb`).
#
# The second half is `String#inspect`, which must ESCAPE a codepoint ruby
# does not consider printable. Which ones those are is Oniguruma's own table
# and follows no rule the general category gives -- U+E000 (private use) and
# U+00AD (soft hyphen) print raw while U+FFFE and every unassigned codepoint
# escape -- so that table is generated from the oracle too
# (`tools/inspect_printable.rb`). A symbol name follows it: a character ruby
# would escape is not an identifier character.
%w[straße ÄB äb äB i I].each_with_index do |s, i|
  p [i, s.upcase(:ascii), s.downcase(:ascii), s.capitalize(:ascii), s.swapcase(:ascii)]
end
p ["ß".downcase(:fold), "ﬁ".downcase(:fold), "Σς".downcase(:fold), "İ".downcase(:fold)]
p ["i".upcase(:turkic), "I".downcase(:turkic), "İ".downcase(:turkic), "ı".upcase(:turkic)]
p ["istanbul".capitalize(:turkic), "iI".swapcase(:turkic), "i".upcase(:turkic, :lithuanian)]
p ["i".upcase(:lithuanian), "I".downcase(:lithuanian), "i".upcase(:lithuanian, :turkic)]
p :äb.upcase(:ascii)
s = "äb".dup
s.upcase!(:ascii)
p s
[proc { "a".upcase(:fold) }, proc { "ß".capitalize(:fold) },
 proc { "i".upcase(:ascii, :turkic) }, proc { "i".upcase(:nope) },
 proc { "i".upcase("ascii") }, proc { "i".upcase(:turkic, :lithuanian, :ascii) },
 proc { "i".upcase(:turkic, :nope) }].each do |f|
  begin
    f.call
  rescue ArgumentError => e
    p e.message
  end
end
p [0xE000.chr(Encoding::UTF_8), 0xFFFE.chr(Encoding::UTF_8), 0xAD.chr(Encoding::UTF_8)]
p [0x378.chr(Encoding::UTF_8), 0x10FFFF.chr(Encoding::UTF_8), 0x1F600.chr(Encoding::UTF_8)]
p "a#{0xFFFE.chr(Encoding::UTF_8)}b"
p 0xFFFE.chr(Encoding::UTF_8).to_sym
p :あ
__END__
[0, "STRAßE", "straße", "Straße", "STRAßE"]
[1, "ÄB", "Äb", "Äb", "Äb"]
[2, "äB", "äb", "äb", "äB"]
[3, "äB", "äb", "äb", "äb"]
[4, "I", "i", "I", "I"]
[5, "I", "i", "I", "i"]
["ss", "fi", "σσ", "i̇"]
["İ", "ı", "i", "I"]
["İstanbul", "İı", "İ"]
["I", "i", "İ"]
:äB
"äB"
"option :fold only allowed for downcasing"
"option :fold only allowed for downcasing"
"too many options"
"invalid option"
"invalid option"
"too many options"
"invalid second option"
["", "\uFFFE", "­"]
["\u0378", "\u{10FFFF}", "😀"]
"a\uFFFEb"
:"\uFFFE"
:あ
