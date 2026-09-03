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
__END__
true
