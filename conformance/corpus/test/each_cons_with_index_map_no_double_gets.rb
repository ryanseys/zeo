# `.each_cons[.with_index].map { ... }` over a side-effecting inner
# array must not re-emit the inner chain. Pre-fix compile_call_expr
# emitted `rc` (the with_index/each_cons placeholder pair) before
# routing to compile_map_expr's fusion path, which then re-emitted
# the inner receiver. The inner block's side effects (`gets`, an
# accumulator method, ...) ran twice -- consuming twice the input
# lines or doubling the counter. Issue #621.

$counter = 0
def step
  $counter += 1
  $counter
end

# `3.times.map { step }` -> [1, 2, 3] (counter should advance by 3,
# not by 6 from a duplicate emission).
result = 3.times.map { step }.each_cons(2).with_index(1).map { |(x, y), i| [x - y, i] }
result.each do |pair|
  puts pair.inspect
end
puts $counter
