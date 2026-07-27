# Ruby hands a slice length straight through from user code, so `a[1, huge]`
# reaches the clamp with len == INT64_MAX. Written as `start + len > N` the sum
# overflows, wraps negative, the guard reads false, and the clamp is skipped --
# after which the copy loop, bounded only by len, walks off the heap. This
# segfaulted on ordinary -O2 for an index CRuby answers fine.
n = ARGV[0].to_i     # runtime value, so nothing folds it away

p(["x", "y", "z"][1, n])
p([1, 2, 3][1, n])
p([1.5, 2.5, 3.5][1, n])
p([1, "x", :y, 2.0][1, n])
p("abcdef".byteslice(1, n))
p("abcdef"[1, n])
p("日本語です"[1, n])

# the boundary cases the clamp still has to get right
p([1, 2, 3][3, n])
p([1, 2, 3][0, n])
p("abc".byteslice(3, n))
p([1, 2, 3][1, 0])
p([1, 2, 3][-2, n])
