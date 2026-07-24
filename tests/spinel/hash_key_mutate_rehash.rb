# Guards the cached-key-hash optimization: a heap string used as a hash
# lookup key caches its hash in the string header. If the string is then
# mutated in place, the cached hash must be invalidated, or a later lookup
# under the new content lands in the wrong bucket and silently misses.
h = {}
h["bar"] = 5

# setbyte: mutate a key whose hash was already cached by a prior lookup
s = "AAA".dup
_miss = (h[s] || 0)        # caches hash("AAA") into s's header
s.setbyte(0, 66)           # s -> "BAA"; cached hash must be cleared
h[s] = 9
puts(h["BAA"])             # fresh hash("BAA") must find 9, not nil
puts(h["BAA".dup])         # same, via a distinct heap pointer

# replace: longer in-place mutation
k = "foo".dup
_m2 = (h[k] || 0)          # caches hash("foo")
k.replace("longerkey")
h[k] = 7
puts(h["longerkey"])       # must find 7

# basic repeated lookup (exercises the cached-hit fast path)
env = {}
env["alpha"] = 1
env["beta"] = 2
total = 0
3.times do
  total += env["alpha"]
  total += env["beta"]
end
puts(total)
