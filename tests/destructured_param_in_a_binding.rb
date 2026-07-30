# A parenthesized destructuring block parameter, in a scope that materializes
# a Binding. Zeo names the un-destructured slot internally (`__destr_0`) and
# listed it among the scope's own locals, which did two wrong things at once:
# `local_variables` reported the synthetic name, and the slot was promoted to
# a shared cell -- while the destructure that READS it runs in the prologue,
# ahead of the promotion, so the read found a plain value where a cell was
# promised and the program did not compile.
#
# prism's lex_compat.rb is written this way (`sort_by.with_index { |(token,
# lex_state), index| ... }`), which is how irb reached it.
seen = nil
[[[1, 2], 9]].each do |(a, b), i|
  seen = binding.local_variables.sort
end
p seen

names = nil
[[1, 2]].each do |(x, y)|
  inner = proc { x + y }
  names = inner.binding.local_variables.sort
end
p names

# The destructure itself still binds, and the Binding still reads the names it
# really does hold.
vals = nil
[[[3, 4], 5]].each do |(m, n), k|
  b = binding
  vals = [b.local_variable_get(:m), b.local_variable_get(:n), b.local_variable_get(:k)]
end
p vals

# A nested destructure, and one with a splat.
deep = nil
[[[1, [2, 3]], 4]].each do |(p1, (p2, p3)), q|
  deep = binding.local_variables.sort
end
p deep

rest = nil
[[[1, 2, 3]]].each do |(first, *others)|
  rest = [binding.local_variables.sort, first, others]
end
p rest

# `Proc#parameters` has always hidden the slot; it agrees with the Binding now.
p(proc { |(a, b), c| }.parameters)
p(lambda { |(a, b), c| }.parameters)
