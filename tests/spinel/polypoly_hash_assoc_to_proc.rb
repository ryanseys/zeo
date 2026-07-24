# PolyPolyHash's order[] holds slot indexes, and its keys are sp_RbVal on the
# proc poly side-channel: Hash#assoc/#rassoc value reads and Hash#to_proc key
# lookups must use the right access (they emitted sp_PolyPolyHash_get(h, index)
# and passed the raw arg slot, a C compile error).

# a default-proc hash is PolyPoly; assoc/rassoc
h = Hash.new { |hh, k| hh[k] = 42 }
h.merge!(foo: :bar)
p h.assoc(:foo)
p h.assoc(42)
p h.rassoc(:bar)

# heterogeneous-key hash to_proc
g = { 1 => "a", :b => "c", "d" => 2 }
pr = g.to_proc
p pr.call(1)
p pr.call(:b)
p pr.call("d")
p [1, :b, "d"].map(&g)
