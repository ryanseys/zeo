# Set.new raises for an Integer, accepts an Array or nothing, and applies its block.
require "set"

r1 = (Set.new(5) rescue $!.class); p r1
p Set.new([1, 2]).to_a
p Set.new.to_a
p Set.new([1, 2, 3]) { |x| x * 2 }.to_a
__END__
ArgumentError
[1, 2]
[]
[2, 4, 6]
