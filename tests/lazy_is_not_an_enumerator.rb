# `Enumerator::Lazy < Enumerator` in ruby. In zeo it is a plain Object that
# includes Enumerable, so the parallel hierarchy is flat and every method a
# lazy shares with an ordinary enumerator has to be re-declared on `Lazy`
# itself. What a program sees is `.owner` and `instance_methods(false)`
# answering `Enumerator::Lazy` where ruby says `Enumerator` or `Enumerable`.
#
# The behaviour those rows carry is already right (`next`/`peek`/`inspect`
# match -- `tests/lazy_external_iteration.rb` is the control), so this is the
# SHAPE alone. Closing it means giving `RLazy` the `EnumeratorData` payload
# every inherited `Enumerator` row downcasts to, which is a different object
# than the source+chain a lazy actually holds.
#
# `zip` comes along for the ride: ruby overrides it on Lazy so it stays lazy,
# while zeo answers the eager `Enumerable#zip` Array.

p Enumerator::Lazy.superclass
p Enumerator::Lazy.ancestors.first(3)

%i[first each to_a inspect size next peek rewind force eager].each do |m|
  puts "#{m}: #{Enumerator::Lazy.instance_method(m).owner}"
end

p (1..3).lazy.zip([1, 2, 3]).inspect
