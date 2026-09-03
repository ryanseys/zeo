# to_int ducks at NUM2LONG index/count sites.
class Two
  def to_int
    2
  end
end

two = Two.new
p [1, 2] * two
p "ab" * two
p [10, 20, 30][two]
p [10, 20, 30].first(two)
p [1, 2, 3].fill(0, two)
p [1, 2, 3, 4].rotate(two)
p [1, 2, 3].dig(two)
p "abcdef"[two]
p [1, 2, 3].combination(two).to_a
p Array.new(two)
__END__
[1, 2, 1, 2]
"abab"
30
[10, 20]
[1, 2, 0]
[3, 4, 1, 2]
3
"c"
[[1, 2], [1, 3], [2, 3]]
[nil, nil]
