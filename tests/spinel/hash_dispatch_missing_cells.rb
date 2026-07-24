# #551. Three additional Hash dispatch cells the prior #510 /
# #546 work hadn't covered: sym_poly_hash#merge(kwarg) /
# str_int_hash#dup / str_poly_hash#fetch. Each one was
# producing a `cannot resolve call to 'X' on <hash_variant>
# (emitting 0)` warning and silently returning 0; this drove
# silent-wrong rendering in Sam's roundhouse blog
# (ActionView's link_to / button_to / content_for_get).

def merge_kwarg(h); h.merge(c: 3); end
m = merge_kwarg({ a: 1, b: "two" })
puts m.length
puts m.has_key?(:a)
puts m.has_key?(:c)

def dup_strint(h); h.dup; end
d = dup_strint({ "a" => 1, "b" => 2 })
puts d.length
puts d["a"]
puts d["b"]

def fetch_strpoly(h, k); h.fetch(k, "miss"); end
hp = { "a" => 1, :b => "two" }
puts fetch_strpoly(hp, "a")
puts fetch_strpoly(hp, "missing")
