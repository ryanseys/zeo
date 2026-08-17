# The containment predicates take a Set and nothing else: CRuby answers
# ArgumentError("value must be a set"). Passing an Array reached the operand's
# own #subset?, which does not exist on one, and refused to compile at all.
require 'set'

r = (Set[1, 2] >= [1] rescue $!.class); p r
r = (Set[1, 2] >= [1] rescue $!.message); p r
r = (Set[1, 2] > [1] rescue $!.class); p r
r = (Set[1, 2] <= [1, 2, 3] rescue $!.class); p r
r = (Set[1, 2] < [1, 2, 3] rescue $!.class); p r
r = (Set[1, 2].subset?([1, 2, 3]) rescue $!.class); p r
r = (Set[1, 2].superset?([1]) rescue $!.class); p r

# a Set operand keeps answering
p(Set[1, 2] >= Set[1])
p(Set[1] <= Set[1, 2])
p(Set[1, 2] > Set[1])
p(Set[1, 2] < Set[1])
p(Set[1, 2].superset?(Set[1, 2]))
p(Set[1, 2].proper_superset?(Set[1, 2]))
