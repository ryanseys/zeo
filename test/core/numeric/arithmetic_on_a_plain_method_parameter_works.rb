# Method params are always statically `Poly` (zeo never infers a
# param's type from call sites) -- this exercises the runtime-checked
# numeric fallback in `clif::expr`, not just literal/local `Int`
# operands.

class Adder
  def add(a, b)
    a + b
  end
end
puts Adder.new.add(3, 4)
__END__
7
