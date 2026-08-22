# `upcase`/`downcase`/`capitalize`/`swapcase` -- and their `!` and `Symbol`
# twins -- accept NO arguments in zeo, so every one of ruby's case-mapping
# options is an `ArgumentError`.
#
# Ruby's options are four, and they are not decoration:
#
#   :ascii        map only `a-z`/`A-Z` and leave every other codepoint
#                 alone. This is what a protocol wants -- an HTTP header
#                 name, a hex digit -- and getting it wrong is a silent data
#                 change, not an error: `"straße".upcase(:ascii)` is
#                 `"STRAßE"`, where the full mapping gives `"STRASSE"` and
#                 the string gets LONGER.
#   :turkic       dotted/dotless I. `"i".upcase(:turkic)` is `"İ"`.
#   :lithuanian   the accent rules, currently only for `i`.
#   :fold         case folding rather than case mapping, for `downcase`.
#
# zeo's rows are declared with no parameters at all, so this is a DSL arity
# question first and a mapping question second: the tables behind the option
# are Unicode's `SpecialCasing.txt` conditional mappings, which the encoding
# work already parses for the unconditional case.
#
# Two or more options may be given together (`:turkic, :lithuanian`), and an
# unknown one is `ArgumentError: invalid option` -- a different message from
# the arity error zeo raises today, which is what makes a caller's `rescue
# ArgumentError` around a probe silently take the wrong branch.
#
# Oracle: every row answers.
%w[straße ÄB äb äB i I].each_with_index do |s, i|
  p [i, s.upcase(:ascii), s.downcase(:ascii), s.capitalize(:ascii), s.swapcase(:ascii)]
end
p "i".upcase(:turkic)
p "I".downcase(:turkic)
p "i".upcase(:lithuanian)
p "İ".downcase(:fold)
p "i".upcase(:turkic, :lithuanian)
p :äb.upcase(:ascii)
s = "äb".dup
s.upcase!(:ascii)
p s
begin
  "i".upcase(:nope)
rescue ArgumentError => e
  p e.message
end
