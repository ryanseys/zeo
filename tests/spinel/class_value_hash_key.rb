# A Class used as a Hash key compares by name, the way `==` on two of them does.
# Hashing the box itself made every occurrence a distinct key: `h[Integer] = 1;
# h[Integer] = 2` kept both, and `group_by(&:class)` made one bucket per element.
h = {}
h[Integer] = 1
h[Integer] = 2
p h.size
p h
p({ Integer => 1, String => 2 }.key?(Integer))
p [1, 2, "a"].group_by { |x| x.class }
p [1, 2, "a"].map(&:class).tally
p [1, 2, "a"].map(&:class).uniq
p({ Integer => "i" }[Integer])
p({ Integer => "i" }.fetch(String, "none"))
counts = Hash.new(0)
[1, "a", 2, :s].each { |v| counts[v.class] += 1 }
p counts
