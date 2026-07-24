# %a/%A hex-float directives render through C's own %a (C99, portable),
# via String#%, format, and sprintf.
p("%a" % 1.0)
p(format("%a", 0.5))
p(sprintf("%A", 2.0))
p("%a" % 255.5)
