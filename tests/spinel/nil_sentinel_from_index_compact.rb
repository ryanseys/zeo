# A miss from String#index / #rindex is nil, and every consumer has to agree:
# an int slot carrying the nil sentinel must box as nil, so compact drops it
# and filter_map treats it as falsy.
v = "a".rindex("/")
w = "abc".index("b")
p v.nil?
p v.class
p [v, 1].compact
p [v, 1].filter_map { |x| x }
p [v, w, 3].compact
p [v, 1].compact.size
p [1, v, 2].compact.sum

# the same for an out-of-range slice on a String array and a Float miss
s = "x"[5, 1]
p [s, "y"].compact
g = [].first
p [g, 1.5].compact

# a live nil in the same position still behaves
p [nil, 1].compact
p [v].compact.empty?
