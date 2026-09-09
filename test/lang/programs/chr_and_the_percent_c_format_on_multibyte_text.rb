# String#chr answers the first character, and `%c` renders a codepoint and a
# character.
# (spinel issue #3083)
p "あいう".chr
p "abc".chr
p "".chr
p "€5".chr
p("%c" % 12354)
p("%c" % "あ")
p("%c" % 65)
p("%c" % "hello")
p("%3c" % 65)
p("%-3c" % 65)
p("[%c]" % 0x1F600)
__END__
"あ"
"a"
""
"€"
"あ"
"あ"
"A"
"h"
"  A"
"A  "
"[😀]"
