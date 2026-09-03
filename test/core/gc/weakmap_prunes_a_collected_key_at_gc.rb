# A key with no other strong reference is pruned once collected.

m = ObjectSpace::WeakMap.new
m[Object.new] = "gone"
GC.start
p m.keys.length
p m.length
# an immediate key is held strongly and never expires; keep the value
# strongly referenced so only key liveness is under test
m2 = ObjectSpace::WeakMap.new
val = Object.new
m2[42] = val
GC.start
p m2[42].equal?(val)
__END__
0
0
true
