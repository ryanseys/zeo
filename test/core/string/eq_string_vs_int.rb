# A String and an Integer are never ==: there is no implicit coercion, so the
# comparison answers false rather than refusing to compile.
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
__END__
false
false
true
true
true
true
false
false
1
