#
# `\u{...}` with more than one codepoint is a PARSE ERROR:
#
#     regex parse error:
#         \u{61 62}
#              ^
#
# CRuby reads a braced `\u{}` as a space-separated LIST of codepoints, so
# `/\u{61 62}/` is `/ab/`. zeo's escape reader takes one and refuses the
# space.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# The engine returned every unknown escape as itself, so \u lost its backslash
# and the rest became literal text: /µ/ named the text "u00b5", and
# /\u{3042}/ read as a quantifier on `u`. Ported from mruby-regexp (048e5da5f).
p("µ" =~ /µ/ ? true : false)
p("u00b5" =~ /µ/ ? true : false)
p("µ" =~ /[µ]/ ? true : false)
p("あ" =~ /\u{3042}/ ? true : false)
p("ab" =~ /\u{61 62}/ ? true : false)
p("abb".match(/\u{61 62}+/)[0])
p("aaa".match(/a+/)[0])
p("a" =~ /[\u{61 62}-z]/ ? true : false)
p("y" =~ /[\u{61 62}-z]/ ? true : false)
p("A" =~ /A/i ? true : false)
p("a" =~ /A/i ? true : false)
p "aあb".gsub(/\u{3042}/, "-")

["\\u", "\\u6", "\\u{}", "\\u{ }", "\\u{d800}", "\\u{110000}", "\\u{0000061}", "\\u{61,62}"]. each do |src|
  begin
    Regexp.new(src)
    puts "#{src}: ok"
  rescue => e
    puts "#{src}: #{e.class}"
  end
end
__END__
true
false
true
true
true
"abb"
"aaa"
true
true
true
true
"a-b"
\u: RegexpError
\u6: RegexpError
\u{}: RegexpError
\u{ }: RegexpError
\u{d800}: RegexpError
\u{110000}: RegexpError
\u{0000061}: RegexpError
\u{61,62}: RegexpError
