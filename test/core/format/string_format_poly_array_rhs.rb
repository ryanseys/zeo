# String#% with a right-hand side of no single type -- a scalar at one call
# site, an Array at another -- spreads an Array value across the format
# directives at runtime, rather than wrapping it as one argument.
def fmt(f, v); f % v; end

# v unifies to poly: it receives an Integer, an Array, a Float, and a String.
p fmt("%d", 5)               # "5"       (scalar stays a one-element list)
p fmt("%d-%d", [1, 2])       # "1-2"     (Array spread across directives)
p fmt("%03d/%02d", [7, 3])   # "007/03"
p fmt("%.1f", 2.5)           # "2.5"
p fmt("%s!", "hi")           # "hi!"
p fmt("%s=%s", ["k", "v"])   # "k=v"     (string array spread)
p fmt("%d %s", [1, "x"])     # "1 x"     (mixed-type array spread)

# A single-element array still fills one directive.
p fmt("[%d]", [9])           # "[9]"
__END__
"5"
"1-2"
"007/03"
"2.5"
"hi!"
"k=v"
"1 x"
"[9]"
