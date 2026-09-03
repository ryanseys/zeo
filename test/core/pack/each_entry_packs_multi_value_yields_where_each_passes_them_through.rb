# The entire difference between `each_entry` and `each`, and it is only
# observable when the receiver's own `each` yields MORE THAN ONE value.

class Multi
  include Enumerable
  def each
    yield 1
    yield 2, 3
    yield
  end
end
r = []
Multi.new.each_entry { |x| r << x }
p r
s = []
Multi.new.each { |x| s << x }
p s
p Multi.new.each_entry.to_a
p [1, 2].each_entry.to_a
h = []
({a: 1}).each_entry { |e| h << e }
p h
__END__
[1, [2, 3], nil]
[1, 2, nil]
[1, [2, 3], nil]
[1, 2]
[[:a, 1]]
