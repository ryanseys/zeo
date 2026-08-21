# A lookbehind was measured by walking its code, and the walk gave up at the
# first alternation -- "for simplicity", though branches of the same width are
# as measurable as a plain sequence. Both branches are measured now and the
# lookbehind is accepted when they agree in characters, which is the unit the
# rewind uses; branches of different widths have no single answer and are still
# refused. Found by a differential fuzzer against CRuby.
def show(src, s)
  m = Regexp.new(src).match(s)
  p(m ? [m.begin(0), m[0]] : nil)
rescue => e
  p e.class
end

show('(?<=a|b)c', "ac")
show('(?<=a|b)c', "bc")
show('(?<=a|b)c', "cc")
show('(?<=ab|cd)e', "abe")
show('(?<=ab|cd)e', "cde")
show('(?<=あ|い)b', "あb")
show('(?<=あ|い)b', "うb")
show('\w+(?<=[^ab]|µ)', "xyz")
show('(?<!a|b)c', "xc")
show('(?<!a|b)c', "ac")
show('(?<=(a|b))c', "ac")
show('(?<=a|b|c)d', "cd")

# Branches of DIFFERENT widths keep no single rewind and are still refused, as
# every other variable-length lookbehind is: `(?<=ab|c)e` raises RegexpError
# here where CRuby matches. That refusal cannot be a line of this test, since
# the expected output is CRuby's.
