# Runtime `super` (#192) across a three-level runtime class chain, a
# `define_method` body, and a `super` inside a `rescue` -- all resolved through
# the runtime method-frame stack because each class is minted at runtime.

a = Class.new do
  def label(s)
    "A(#{s})"
  end
end
b = Class.new(a) do
  def label(s)
    "B->" + super
  end
end
c = Class.new(b) do
  # define_method body super, forwarding the same arg up the chain
  define_method(:label) { |s| "C->" + super(s) }
end
p c.new.label("x")

# super inside a rescue in a runtime method still targets the runtime parent
guard = Class.new(a) do
  def label(s)
    raise "boom" if s.empty?
    super
  rescue
    "A(<empty>)"
  end
end
p guard.new.label("ok")
p guard.new.label("")

# a native Struct with a custom initialize that supers, then reads back
weighted = Struct.new(:base, :bonus) do
  def initialize(base)
    super(base, base / 2)
  end
  def total
    base + bonus
  end
end
w = weighted.new(10)
p [w.base, w.bonus, w.total]
__END__
"C->B->A(x)"
"A(ok)"
"A(<empty>)"
[10, 5, 15]
