# each_with_object accumulator element type is inferred from how the memo is
# filled (`memo << e`), even when the seed is an empty `[]`. The inference
# follows a forwarded callable value into its body, so `&proc` / `&->(){}` /
# `&method(:m)` accumulate the right element type too. Distinct memo-param names
# keep sibling blocks from sharing a typed C local; receivers route through a
# method param to defeat constant folding.
def ia(x) = x

# --- literal blocks: empty [] seed, element type taken from the push ---
p ia([1, 2, 3]).each_with_object([]) { |x, mi| mi << x * 2 }       # [2, 4, 6]
p ia([1, 2, 3]).each_with_object([]) { |x, ms| ms << x.to_s }      # ["1", "2", "3"]
p ia([1, 2, 3]).each_with_object([]) { |x, mf| mf << x / 2.0 }     # [0.5, 1.0, 1.5]
p ia([1, 2, 3]).each_with_object([]) { |x, mp| mp << x; mp << x.to_s } # [1, "1", 2, "2", 3, "3"]
p ia([1, 2, 3]).each_with_object([]) { |x, mu| mu.push(x.to_s) }   # ["1", "2", "3"] (push)
p ia([1, 2, 3]).each_with_object([]) { |x, mn| mn }                # [] (no push -> empty)

# --- non-empty typed seed keeps working ---
p ia([1, 2, 3]).each_with_object(["seed"]) { |x, mt| mt << x.to_s } # ["seed", "1", "2", "3"]

# --- forwarded callable values: body-of-callable inference ---
add_dbl = ->(x, acc) { acc << x * 2 }
p ia([1, 2, 3]).each_with_object([], &add_dbl)                     # [2, 4, 6]
add_str = ->(x, acc) { acc << x.to_s }
p ia([1, 2, 3]).each_with_object([], &add_str)                    # ["1", "2", "3"]
p ia([1, 2, 3]).each_with_object([], &->(x, acc) { acc << x * 10 }) # [10, 20, 30]
p ia([1, 2, 3]).each_with_object([], &->(x, acc) { acc << x.to_s }) # ["1", "2", "3"]

def collect_s(x, acc) = acc << x.to_s
p ia([1, 2, 3]).each_with_object([], &method(:collect_s))         # ["1", "2", "3"]

# --- assigned result keeps a precise type (no fixpoint widening to poly) ---
r = ia([1, 2, 3]).each_with_object([], &add_str)
p r                                                               # ["1", "2", "3"]
