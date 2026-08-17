class Money
  def initialize(cents)
    @cents = cents
  end

  def to_i
    @cents
  end

  def to_f
    @cents / 100.0
  end
end

# a mixed poly container: the user conversion and the builtin one both answer
[Money.new(250), 7, "9", 3.7].each do |v|
  p v.to_i
  p v.to_f
end

# a poly slot whose user method returns a value that outgrew Integer
class Big
  def initialize(v)
    @v = v
  end

  def to_i
    @v & 0xFFFFFFFFFFFFFFFF
  end
end

def pick(f)
  f ? Big.new(-5) : nil
end

p pick(true).to_i
p(-5 & 0xFFFFFFFFFFFFFFFF)
p(-1 | 0xFFFFFFFFFFFFFFFF)
p(-2 & (2**70 - 1))
p "big=#{2**80}"
