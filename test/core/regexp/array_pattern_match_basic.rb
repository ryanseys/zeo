# Issue #669: `case arr in [a, b, c]` pattern matching with
# LocalVariableTargetNode bindings. Pre-fix, probe_parse emitted
# `UnsupportedNode { kind: "ArrayPatternNode" }` and the codegen
# silently ran the body with lv_a / lv_b / lv_c never declared
# (C compile failed with "undeclared identifier"). Fix wires the
# pattern through probe_parse + compile_array_pattern_arm so each
# bound LV gets declared with the scrutinee's elem type and
# initialised from the matching index.

case [1, 2, 3]
in [a, b, c]
  puts "matched: #{a}, #{b}, #{c}"
end

# String array
case ["x", "y", "z"]
in [p, q, r]
  puts "matched: #{p}, #{q}, #{r}"
end

# Non-match arm: fall through to else
case [1, 2]
in [a, b, c]
  puts "wrong"
in [a, b]
  puts "two: #{a}, #{b}"
end
__END__
matched: 1, 2, 3
matched: x, y, z
two: 1, 2
