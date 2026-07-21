# Hash#to_h with no block is the identity (returns self). Verified via class,
# length, and key access to avoid the 3.2-vs-3.4 hash-inspect spacing diff.
h = {a: 1, b: 2}
g = h.to_h
puts g.class
puts g.length
puts g[:a]
puts g[:b]

s = {"x" => 10, "y" => 20}
puts s.to_h[:x] if false   # string-keyed
t = s.to_h
puts t["x"]
puts t["y"]
puts t.length

n = {1 => 100}.to_h
puts n[1]
puts "done"
