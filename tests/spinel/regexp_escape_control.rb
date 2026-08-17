# Regexp.escape renders a control character as its LETTER form -- a backslash
# and an 'n' -- where a backslash followed by the real newline came out.
p(Regexp.escape("a\nb"))
p(Regexp.escape("a\tb"))
p(Regexp.quote("a\rb"))
p(Regexp.escape("a b"))
p(Regexp.escape("a.b"))
p(Regexp.escape("a\fb"))
p(Regexp.escape("a\vb"))
p(Regexp.escape("a*b+c"))
p("x" =~ Regexp.new(Regexp.escape("x")))
