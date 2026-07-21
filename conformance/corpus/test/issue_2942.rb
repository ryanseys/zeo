# Calling a Proc obtained via Array#last: `.last` must be a self-contained
# expression (not a g_pre decl) so it works as the receiver of a following
# `.call`.
pipe = [->(x) { x * 2 }, ->(x) { x + 1 }]
p pipe.last.call(6)
p pipe.first.call(6)
fns = [->(a, b) { a + b }, ->(a, b) { a * b }]
p fns.last.call(3, 4)
