# Struct#to_h { |k, v| [nk, nv] } whose new key is neither a Symbol nor a
# String fell back to a string-keyed hash, so the key went into a const char *
# slot and the C did not compile.
P = Struct.new(:x, :y)
p(P.new(1,2).to_h { |k,v| [v,k] })
p(P.new(1,2).to_h)
p(P.new(1,2).to_h { |k,v| [k.to_s, v] })
p(P.new(1,2).to_h { |k,v| [k, v*2] })
