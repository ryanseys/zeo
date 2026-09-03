# to_ary ducks at Array-argument sites.
class Pair
  def to_ary
    [8, 9]
  end
end

pair = Pair.new
p [1] + pair
p [8, 1] & pair
p [8, 1] - pair
p [1] | pair
p [1].concat(pair)
p [1, 2].zip(pair)
p [[1, 2], pair].transpose
p [1].product(pair)
p [1].replace(pair)
p [8, 7].intersect?(pair)
__END__
[1, 8, 9]
[8]
[1]
[1, 8, 9]
[1, 8, 9]
[[1, 8], [2, 9]]
[[1, 8], [2, 9]]
[[1, 8], [1, 9]]
[8, 9]
true
