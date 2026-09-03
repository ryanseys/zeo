# `3.times`, `(a..b).each` and `arr.each` fuse into a counted loop when the
# block takes at most one required parameter. Ruby binds every other shape
# happily -- `3.times { |a, b| }` gives `b` nil -- so those take the ordinary
# block send instead of being refused.
#
# The emitter used to check the parameter shape AFTER deciding to splice, and
# refused what it found. Found 2026-08-21 while looking for a live refusal to
# pin a diagnostics test on.
3.times { |a, b| p [a, b] }
(1..3).each { |a, b| p [a, b] }
[10, 20].each { |a, b| p [a, b] }

3.times { |a,| p a }
3.times { |*a| p a }
3.times { |a, (b, c)| p [a, b, c] }
3.times { |a, *rest| p [a, rest] }
3.times { |a, b: 9| p [a, b] }
3.times { |a, &blk| p [a, blk] }

[[1, 2], [3, 4]].each { |a, b| p [a, b] }
[[1, 2], [3, 4]].each { |(a, b)| p [a, b] }

# The one-parameter shapes still fuse, and answer the same.
3.times { |i| p i }
(1..3).each { |i| p i }
[7, 8].each { |e| p e }
3.times { p :none }

# The loop's own value survives either path.
p 3.times { |a, b| }
p((1..3).each { |a, b| })
p [1, 2].each { |a, b| }

# A block-local and a closure capture, on the non-fused path.
seen = []
3.times { |a, b; tmp| tmp = a * 2; seen << tmp }
p seen
__END__
[0, nil]
[1, nil]
[2, nil]
[1, nil]
[2, nil]
[3, nil]
[10, nil]
[20, nil]
0
1
2
[0]
[1]
[2]
[0, nil, nil]
[1, nil, nil]
[2, nil, nil]
[0, []]
[1, []]
[2, []]
[0, 9]
[1, 9]
[2, 9]
[0, nil]
[1, nil]
[2, nil]
[1, 2]
[3, 4]
[1, 2]
[3, 4]
0
1
2
1
2
3
7
8
:none
:none
:none
3
1..3
[1, 2]
[0, 2, 4]
