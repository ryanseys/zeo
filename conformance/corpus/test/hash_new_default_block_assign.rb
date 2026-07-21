# Hash.new with a default block whose final expression is a value-producing
# `h[k] = v` must store and return the assigned value (Ruby: `(h[k]=v)` => v).
h = Hash.new { |hash, k| hash[k] = 0 }
h[:a] += 1
puts h[:a]
puts h[:missing]

hs = Hash.new { |hash, k| hash[k] = "missing" }
puts hs[:x]
puts hs[:x]

# The explicit non-final workaround must keep working.
hw = Hash.new { |hash, k| hash[k] = 0; 0 }
hw[:a] += 1
puts hw[:a]
puts hw[:missing]
