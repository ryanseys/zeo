class Box
  def run
    self.helper + implicit_helper
  end

  private

  def helper
    10
  end

  def implicit_helper
    helper
  end
end

puts Box.new.run

class A
  def pub_a
    priv_a
  end

  private def priv_a
    "priv_a"
  end
end

class B
  def pub_b
    priv_b
  end

  def priv_b
    "priv_b"
  end

  private :priv_b
end

puts A.new.pub_a
puts B.new.pub_b

class Money
  def initialize(amount)
    @amount = amount
  end

  def greater_than_five
    other = Money.new(5)
    amount > other.amount
  end

  protected

  def amount
    @amount
  end
end

puts Money.new(10).greater_than_five

class Secretive
  private

  def secret
    "shh"
  end
end

puts Secretive.new.send(:secret)
__END__
20
priv_a
priv_b
true
shh
