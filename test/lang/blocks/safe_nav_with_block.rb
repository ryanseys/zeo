# A block on a safe-navigation call (`recv&.m { ... }`): nil short-circuits
# before the block is entered, else the block rides the ordinary dispatch.
a = [1, 2, 3]
puts a&.map { |x| x * 2 }.inspect
b = nil
puts (b&.each { |x| puts x }).inspect
h = {}
[[:a, 1], [:b, 2]].each { |k, v| h[k] = v }
puts h&.select { |_, v| v > 1 }.inspect
__END__
[2, 4, 6]
nil
{b: 2}
