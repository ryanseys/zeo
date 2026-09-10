# &, |, merge, subtract and - each raise for a non-enumerable operand.
require "set"

r1 = (Set[1, 2] & 5 rescue $!.class); p r1
r2 = (Set[1, 2] | 5 rescue $!.class); p r2
r3 = (Set[1, 2].merge(5) rescue $!.class); p r3
r4 = (Set[1, 2].subtract(5) rescue $!.class); p r4
r5 = (Set[1, 2] - 5 rescue $!.class); p r5
p((Set[1, 2] & [2, 3]).to_a)
p((Set[1, 2] | [3]).to_a)
__END__
ArgumentError
ArgumentError
ArgumentError
ArgumentError
ArgumentError
[2]
[1, 2, 3]
