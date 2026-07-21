# #690: Symbol-keyed hash literal passed to a sym_poly_hash-typed
# callee param must convert at the call boundary; the post-#676
# kwargs-bind path no longer covered this shape.
#
# Mirrors the failing roundhouse pattern: render_attrs takes
# Hash[Symbol, untyped] (sym_poly_hash), callers pass a literal
# {key: val} (sym_str_hash). The value types diverge, so the call
# needs sp_SymStrHash_to_sym_poly to bridge.

def render_attrs(attrs)
  pairs = []
  attrs.each do |k, v|
    pairs << k.to_s + "=" + v.to_s
  end
  pairs.join(" ")
end

# Calling with a Symbol-keyed string-valued literal.
puts render_attrs({ class: "primary", id: "go" })

# Calling with the merged form: literal SymStr + LV SymStr.
def build_attrs
  inner = { id: "foo" }
  render_attrs({ class: "btn" }.merge(inner))
end

puts build_attrs
