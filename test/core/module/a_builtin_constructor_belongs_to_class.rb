# `new` on these builtins is `Class#new` in ruby: none of them lists it as
# its own singleton method.
CLASSES = [
  Dir, Enumerator, Fiber, Random, Set, Thread::Queue, Thread::SizedQueue,
  Thread::ConditionVariable, ThreadGroup, Ractor::Port, File::Stat,
  Enumerator::Chain, Enumerator::Generator, Enumerator::Lazy, Enumerator::Product
].freeze
p CLASSES.map { _1.singleton_methods(false).include?(:new) }.uniq
p CLASSES.map { _1.method(:new).owner.to_s }.uniq
p Set.new([1, 2]).size, Random.new(4).class, Thread::Queue.new.size
p Dir.new(".").class, Enumerator.new { |y| y << 1 }.to_a
__END__
[false]
["Class"]
2
Random
0
Dir
[1]
