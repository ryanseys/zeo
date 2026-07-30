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
