# Hash#default(key) invokes the default_proc with (self, key) when the hash
# was built with a block; default() with no key, or a value-default hash,
# returns the stored default object.
h = Hash.new { |hash, k| k.to_s }
p h.default(:foo)
p h.default
g = Hash.new(99)
p g.default(:x)
p g.default
