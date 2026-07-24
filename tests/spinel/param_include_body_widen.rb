# #558 (rubys / Sam Ruby). Sibling of #552 for `.include?`,
# the other "ambiguous receiver" method that exists on
# String / Array / Hash / Range but NOT on Integer.
# Pre-fix: `cols.include?(:updated_at)` on an
# int/nil-defaulted param surfaced as "cannot resolve call to
# 'include?' on int" and silently returned 0.
#
# Post-fix: is_collection_query_method also matches `include?`,
# so the param widens to flat `poly`. emit_poly_builtin_dispatch
# emits include? arms across the relevant Array / Hash variants
# keyed by the arg type (Hash#include? aliases has_key?).
# SP_TAG_STR include? fires when the arg is a string (substring
# search per CRuby's String#include? semantics).

def consume(coll)
  coll.include?(:x)
end

class Box
  attr_accessor :v
end

b = Box.new
puts consume(b.v)
b.v = [:y, :x, :z]
puts consume(b.v)
b.v = [:a, :b]
puts consume(b.v)
b.v = {x: 1, y: 2}
puts consume(b.v)
b.v = "abc"
puts consume(b.v)
