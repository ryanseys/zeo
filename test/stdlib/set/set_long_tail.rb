# Two Set rows: the FrozenError names the receiver ("can't modify
# frozen Set: Set[1]"), and a binary op on a Set SUBCLASS answers the
# subclass.
require "set"
begin
  Set[1].freeze.add(2)
rescue FrozenError => e
  puts e.message
end
k = Class.new(Set)
p (k[1] | Set[2]).class == k
__END__
can't modify frozen Set: Set[1]
true
