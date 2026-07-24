# String#% with a bare nil / true / Rational operand: scalars box into
# the one-element format path, and the float directives coerce a boxed
# Rational (%.2f % (1r/3) => "0.33").
p("x=%s" % nil)
p("%.2f" % (1r/3))
p("%s!" % true)
p("%d" % 5)
