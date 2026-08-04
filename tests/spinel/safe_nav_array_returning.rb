# `x&.keys` / `&.values` / `&.to_a` on a receiver that is not statically a Hash
# typed the whole expression poly, while the guarded call itself lowers to an
# sp_PolyArray *. The C conditional then had an sp_RbVal nil against a pointer
# and the build aborted. An array result is a pointer whose NULL already reads
# as nil, so the guard can stay in pointer form. #3461.
x = nil
p(x&.keys)
p(x&.values)
p(x&.to_a)
rows = [{ "a" => 1 }]
p(rows.first&.keys)
p(rows.first&.values)
p(rows.first&.to_a)
h = { "a" => 1 }
p(h&.keys)
p(rows.first&.size)
r2 = [["x", "y"]]
p(r2.first&.reverse)
empty = []
p(empty.first&.keys)
