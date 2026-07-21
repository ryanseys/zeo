# Issue #720 follow-up: bare `super` (ForwardingSuperNode) inside
# a prepended method forwards every formal parameter to the
# shadow, same as the explicit form.
module M
  def add(a, b)
    super + 100
  end
end

class C
  def add(a, b)
    a + b
  end
  prepend M
end

puts C.new.add(2, 3)
