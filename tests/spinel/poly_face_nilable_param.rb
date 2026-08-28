# A parameter that may also receive nil boxes its value, and the boxed face has
# to answer the names the real class does: Hash#key, the Array#fill mutator, and
# the two-argument String#index/#rindex all raised NoMethodError (#4149).
def hash_key(x) = x.key(1)
def arr_fill(x) = x.fill(9)
def str_index(x) = x.index("b", 2)
def str_rindex(x) = x.rindex("b", 3)
def str_index_miss(x) = x.index("z", 1)

p hash_key(true ? { "a" => 1 } : nil)

a = [1, 2, 3, 4]
p arr_fill(true ? a : nil)
p a   # fill mutates the receiver, not a copy

p str_index(true ? "abcabc" : nil)
p str_rindex(true ? "abcabc" : nil)
p str_index_miss(true ? "abcabc" : nil)
