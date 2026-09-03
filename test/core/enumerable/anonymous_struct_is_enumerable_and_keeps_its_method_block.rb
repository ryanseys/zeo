# A native anonymous struct is Enumerable and honours a class-body method
# block, exactly like the synthesized constant form.

k = Struct.new(:a, :b, :c) do
  def sum
    to_a.sum
  end
end
o = k.new(1, 2, 3)
p o.map { |v| v * 10 }
p o.sum
p o.select { |v| v.odd? }
__END__
[10, 20, 30]
6
[1, 3]
