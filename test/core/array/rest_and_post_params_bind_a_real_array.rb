class Collector
  def count(*nums)
    nums.length
  end
  def between(a, *mid, z)
    "#{a}-#{mid.length}-#{z}"
  end
end
c = Collector.new
puts c.count(1, 2, 3)
puts c.count
puts c.between(1, 2, 3, 9)
__END__
3
0
1-2-9
