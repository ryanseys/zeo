# count { |x| <poly value> } used the block value directly as a C condition;
# a poly (sp_RbVal) predicate is a struct, invalid in `if (...)` (#1508).
# Routing it through emit_cond makes any predicate a valid truthiness test.
items = [1, nil, "x", false, 2, true]
p items.count { |x| x }              # truthy: 1, "x", 2, true -> 4
p items.count { |x| x.nil? }         # 1
mixed = [1, "two", 3, "four", 5]
p mixed.count { |x| x.is_a?(Integer) }
