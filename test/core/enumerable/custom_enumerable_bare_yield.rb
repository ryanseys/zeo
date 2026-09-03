class Nums
  include Enumerable
  def each
    yield 3
    yield 1
    yield 2
  end
end
n = Nums.new
p n.to_a
p n.sort
p n.map { |x| x * 10 }
p n.include?(2)
__END__
[3, 1, 2]
[1, 2, 3]
[30, 10, 20]
true
