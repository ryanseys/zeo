# A builtin answering a method its ancestor owns reports the ancestor, as
# ruby does -- the row is here for dispatch, not for reflection.
PAIRS = [
  [Set, :clone], [Set, :dup], [Range, :sum], [Time, :==], [Binding, :inspect],
  [Binding, :to_s], [Dir, :to_s], [Enumerator, :to_s], [IO::Buffer, :==],
  [File::Stat, :to_s]
].freeze
PAIRS.each { p [_1.to_s, _2, _1.instance_method(_2).owner.to_s] }
# ...and the ones each class really owns stay its own.
OWN = [[Dir, :inspect], [Enumerator, :inspect], [File::Stat, :inspect], [Set, :inspect], [Time, :eql?]].freeze
OWN.each { p [_1.to_s, _2, _1.instance_method(_2).owner.to_s] }
p Set[1, 2].dup.to_a, (1..4).sum, Time.at(0) == Time.at(0), Time.at(0).eql?(Time.at(0))
p Dir.new(".").inspect.start_with?("#<Dir"), Set[1].inspect
__END__
["Set", :clone, "Kernel"]
["Set", :dup, "Kernel"]
["Range", :sum, "Enumerable"]
["Time", :==, "Comparable"]
["Binding", :inspect, "Kernel"]
["Binding", :to_s, "Kernel"]
["Dir", :to_s, "Kernel"]
["Enumerator", :to_s, "Kernel"]
["IO::Buffer", :==, "Comparable"]
["File::Stat", :to_s, "Kernel"]
["Dir", :inspect, "Dir"]
["Enumerator", :inspect, "Enumerator"]
["File::Stat", :inspect, "File::Stat"]
["Set", :inspect, "Set"]
["Time", :eql?, "Time"]
[1, 2]
10
true
true
true
"Set[1]"
