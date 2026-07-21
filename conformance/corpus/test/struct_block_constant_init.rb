# A constant assigned inside a `Struct.new(...) do ... end` block body must be
# initialized. register_structs consumes the block only for class/ivar/method
# registration, never as executable statements, so such a constant stayed
# NULL/default -- a method reading it saw an empty value (doom's Linedef
# FLAGS[:TWOSIDED] came back nil, so every linedef read as one-sided and the
# two-sided portal walls never rendered).
Line = Struct.new(:flags) do
  FLAGS = {
    BLOCKING: 0x0001,
    TWOSIDED: 0x0004,
    SECRET: 0x0020
  }.freeze
  def two_sided?
    (flags & FLAGS[:TWOSIDED]) != 0
  end
  def flag(name)
    FLAGS[name]
  end
end
puts Line::FLAGS[:TWOSIDED]
l = Line.new(0x0004)
puts l.two_sided?
puts Line.new(0x0001).two_sided?
puts l.flag(:SECRET)
