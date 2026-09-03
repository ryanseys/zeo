m = ObjectSpace::WeakMap.new
p m.class
k1 = "a"; k2 = "b"
v1 = Object.new; v2 = Object.new
m[k1] = v1
m[k2] = v2
p m[k1].equal?(v1)
p m.key?(k1)
p m.include?(k2)
p m.member?("absent")
p m.length
p m.size
p m.keys.length
p m.values.length
# identity keying: an equal-but-different key string misses
p m["a"].nil?
# overwrite in place, not a second entry
m[k1] = v2
p m.length
p m[k1].equal?(v2)
# delete returns the value, then the key is gone
p m.delete(k1).equal?(v2)
p m[k1].nil?
p m.length
__END__
ObjectSpace::WeakMap
true
true
true
false
2
2
2
2
true
2
true
true
true
1
