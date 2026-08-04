# An `Enumerator::Lazy` IS an Enumerator, so it answers `next`/`peek` and
# iterates one element at a time on demand. zeo's lazy has no `next`
# (`NoMethodError`), which removes the only way to consume an infinite lazy
# chain incrementally rather than in `first(n)`-sized bites.
#
# `inspect` is the same omission seen from outside: ruby describes the chain it
# has built -- `#<Enumerator::Lazy: #<Enumerator::Lazy: 1..3>:map>` -- so a
# lazy value printed in a log or a debugger says what it will produce. zeo
# answers a bare `#<Enumerator::Lazy>` for every one.
#
# The eager Enumerator already has both (`[1,2].each.next` and its inspect both
# answer correctly below), so the machinery exists and Lazy does not reach it.

l = (1..).lazy.map { _1 * 2 }
p l.next
p l.next
p l.peek

p (1..3).lazy.map { _1 }.inspect
p (1..3).lazy.select(&:odd?).inspect

# The eager side already works.
e = [1, 2].each
p [e.next, e.peek]
p [1, 2].each.inspect

# ...and the bulk lazy operations are fine.
p (1..Float::INFINITY).lazy.select(&:even?).first(3)
