# `def f(x = nil)` should keep nil observable at the no-arg call
# site, even when other call sites supply a non-nullable scalar
# (symbol / int / float / bool). Pre-fix the per-call-site
# unifier picked the typed `at` over the `nil` default, so the
# param's C storage was the scalar type and `f()` defaulted the
# missing arg to that scalar's zero value (`sp_sym 0`, `mrb_int
# 0`, etc.) — the user's `x.nil?` branch became dead code.
# Issue #634 shape A.

def f(x = nil)
  if x.nil?
    "branch: nil"
  else
    "branch: not-nil (#{x.to_s})"
  end
end

puts f()
puts f(:foo)

def g(n = nil)
  n.nil? ? "no-int" : "int=#{n.to_s}"
end

puts g()
puts g(42)
