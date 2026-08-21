# A lookbehind was measured and rewound in BYTES, so a class or a `.` inside
# one -- each of which matches a character of whatever width -- landed the
# rewind in the middle of a character (a silent no-match), and `.` was refused
# outright for having no fixed byte length. Measuring and rewinding in
# characters is what CRuby does. Ported from mruby-regexp (103e1a8bc,
# 3df2926a1, bd21fe4aa).
p("あb" =~ /(?<=[あい])b/ ? true : false)
p("ab" =~ /(?<=[ab])b/ ? true : false)
p("ab" =~ /(?<=[^x])b/ ? true : false)
p("あb" =~ /(?<=[^x])b/ ? true : false)
p("ab" =~ /(?<=.)b/ ? true : false)
p("あb" =~ /(?<=.)b/ ? true : false)
p("あいb" =~ /(?<=あい)b/ ? true : false)
p("あb" =~ /(?<!あ)b/ ? true : false)
p("いb" =~ /(?<!あ)b/ ? true : false)
p "aあb".sub(/(?<=あ)b/, "-")
p "aあbあb".gsub(/(?<=あ)b/, "-")
p("あいうb".match(/(?<=..)うb/) ? true : false)
p "xyz".match(/(?<=xy)z/)[0]
p("b" =~ /(?<=a)b/ ? true : false)
