# A String and an Integer are never == in Ruby (no implicit coercion).
# Spinel used to lock the == operand types and emit sp_str_eq(str, int)
# (or int == char*), failing to compile.
p("3" == 3)
p(3 == "3")
p("3" != 3)
p(3 != "3")
# Same-type comparisons still behave normally.
p("3" == "3")
p(3 == 3)
p("3" == "4")

# Both operands are still evaluated (side effects preserved).
$c = 0
def bump
  $c = $c + 1
  "x"
end
p(bump == 5)
p $c
