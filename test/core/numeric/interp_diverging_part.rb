# An uninitialized constant inside interpolation raises NameError. The hazard
# is a compiler that closes the raise with a placeholder of one type while the
# interpolation reads the node as another
# was handed to sp_poly_to_s (#4092).
#
# Anything that diverges is discarded and answers the empty string instead. The
# raise means nothing reads it.

module Engine
end

def cost
  "cost #{Engine::DEFAULT_COST}"
end

def try
  yield
rescue NameError => e
  e.class
end

p try { cost }

# a reachable interpolation still works, and is not disturbed by the probe that
# looks for the raise (it emits the value once and hands the text on)
X = 3
def fine = "n=#{X} s=#{X.to_s} f=#{1.5} a=#{[1, 2]}"
p fine

i = 0
def bump(n) = n + 1
s = "#{bump(i)}#{bump(i)}"
p s
p i
__END__
NameError
"n=3 s=3 f=1.5 a=[1, 2]"
"11"
0
