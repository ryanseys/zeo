# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# `f = ->(n) { memo[n] ||= f.call(...) }` -- a lambda reaching itself through a captured local.
#@ gccheck: cycle leak: 4 objects (cell x2, Hash x1, Proc x1)
# `h[k] ||= rhs` must not evaluate any part of rhs when the key is present:
# a memoizing lambda whose rhs calls itself twice otherwise recurses forever.
memo = { 0 => 0, 1 => 1 }
f = nil
f = ->(n) { memo[n] ||= f.call(n - 1) + f.call(n - 2) }
p f.call(2)
p f.call(10)
p memo[10]

# &&= on the same shape
seen = { "a" => 1 }
g = ->(k) { seen[k] &&= seen[k] + 100 }
p g.call("a")
p g.call("b")
p seen

# a string-keyed hash, and a plain local ||= with a call on the right
counts = {}
def bump(h, k)
  h[k] ||= expensive(k)
end
def expensive(k)
  puts "computing #{k}"
  k.length
end
p bump(counts, "xy")
p bump(counts, "xy")
p counts

arr = [nil, 2]
arr[0] ||= 41
p arr
__END__
1
55
55
101
nil
{"a" => 101}
computing xy
2
2
{"xy" => 2}
[41, 2]
