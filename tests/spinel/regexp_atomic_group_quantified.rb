# Inserting a quantifier's SPLIT ahead of an atom relocates every jump target
# that crosses the insertion, and the atomic group carries one in the same
# field as the lookarounds do. It was left out of that walk, so `(?>a?)*` kept
# the target the atom had before the insertion -- one short, which is the
# sub-pattern's own MATCH. The match ended there, and the outer SAVE that
# records where group 0 ends never ran.
def show(re, s)
  m = re.match(s)
  p(m ? [m.begin(0), m.end(0), m[0]] : nil)
end

show(/(?>a?)*/, "b")
show(/(?>a?)*/, "a")
show(/(?>a?)*b/, "b")
show(/x(?>a?)*/, "xb")
show(/(?>a?)+/, "b")
show(/(?>a?)?/, "b")
show(/(?>a?){2}/, "b")
show(/(?>a*)*/, "aab")
show(/(?>ab|a)*c/, "abac")
show(/(?>\d+)?x/, "12x")
p "aaab".match(/(?>a*)ab/)
p "aaa".scan(/(?>a)?/).size
