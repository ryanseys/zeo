# Array/Enumerable#to_h with a block maps each element to a [key, value] pair
# and collects the pairs into a hash.
p [1, 2, 3].to_h { |x| [x, x * x] }
p %w[a bb ccc].to_h { |s| [s, s.length] }
p (1..3).to_h { |n| [n.to_s, n] }
p({a: 1, b: 2}.to_h { |k, v| [k, v * 10] })
p ["a", "b"].each_with_index.to_h { |s, i| [s, i] }

# blockless to_h still builds a hash from an array of pairs
p [[1, 2], [3, 4]].to_h

h = [1, 2].to_h { |x| [x, x + 1] }
p h[2]
__END__
{1 => 1, 2 => 4, 3 => 9}
{"a" => 1, "bb" => 2, "ccc" => 3}
{"1" => 1, "2" => 2, "3" => 3}
{a: 10, b: 20}
{"a" => 0, "b" => 1}
{1 => 2, 3 => 4}
3
