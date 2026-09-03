m = ObjectSpace::WeakMap.new
a = Object.new; b = Object.new
m[a] = 1
m[b] = 2
vals = []
m.each { |k, v| vals << v }
p vals.sort
keys_seen = 0
m.each_key { |k| keys_seen += 1 }
p keys_seen
vs = []
m.each_value { |v| vs << v }
p vs.sort
pairs = 0
m.each_pair { |k, v| pairs += 1 }
p pairs
# empty map inspect starts with the byte-exact CRuby prefix
empty = ObjectSpace::WeakMap.new
p(empty.inspect.start_with?("#<ObjectSpace::WeakMap:0x"))
p empty.inspect.end_with?(">")
__END__
[1, 2]
2
[1, 2]
2
true
true
