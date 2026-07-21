# Sibling blocks reusing a parameter name must not collapse onto one
# typed slot. Block params intern into the enclosing (flat) scope, so two
# blocks both named |x| of different element types unified to poly and
# corrupted codegen. The alpha-rename pass now splits genuine collisions.

# --- chained maps of divergent element types (the original report) ---
# [1,2,3] -> float -> int. The two |x| are int and float respectively.
p [1, 2, 3].map { |x| x + 0.5 }.map { |x| x.floor }
# [1, 2, 3]

# --- the same chain folded with reduce (the exact filed one-liner) ---
# The second map's |x| must re-narrow poly->float so the chain yields a
# typed int array that reduce can fold, instead of locking poly.
puts([1, 2, 3].map { |x| x + 0.5 }.map { |x| x.floor }.reduce(0) { |a, x| a + x })
# 6

# --- a genuinely heterogeneous receiver must stay poly (no bad re-narrow) ---
p [1, "a", 2.0].map { |x| x.inspect }.reduce("") { |a, x| a + x }
# "1\"a\"2.0"

# --- three sibling each_index blocks share |i| (all int indices) ---
def t_each_index
  [10, 20, 30].each_index { |i| puts i }
  puts "---"
  %w[a b c].each_index { |i| puts "idx #{i}" }
  puts "---"
  [].each_index { |i| puts "never" }
end
t_each_index

# --- sibling selects of divergent element types share |v| ---
def t_select
  ints = [1, 2, 3, 4].select { |v| v > 2 }
  strs = %w[aa b ccc].select { |v| v.length > 1 }
  puts ints.inspect
  puts strs.inspect
end
t_select

# --- nested same-name: inner |x| shadows the outer block's |x| ---
[1].each { |x| [20, 30].each { |x| puts x } }
# 20
# 30

# --- nested capture: inner block (no param) captures the outer |x| ---
[7].each { |x| [1, 2].each { puts x } }
# 7
# 7

# --- inject folds reusing |acc, e| over arrays of different element types ---
def t_inject
  si = [1, 2, 3].inject(0) { |acc, e| acc + e }
  ss = %w[a b c].inject("") { |acc, e| acc + e }
  puts si
  puts ss
end
t_inject
