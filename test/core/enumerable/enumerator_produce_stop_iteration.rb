# `StopIteration` raised inside a block that an Enumerator is driving ENDS the
# enumeration; it is not an error. zeo lets it escape from
# `Enumerator.produce`, so the idiomatic "generate until done" loop aborts the
# program instead of returning what it produced.
#
# `Enumerator.produce` exists to build a sequence from a step function, and
# raising `StopIteration` from that function is the documented way to say the
# sequence has ended (it is the only way -- `produce` has no length and no
# terminating predicate). Ruby catches it in the enumeration driver, the same
# place `loop` catches it.
#
# zeo already handles the sibling case: `loop` DOES stop on a StopIteration
# from `next`, which is the line at the end here. What is missing is the same
# catch around the block `produce` calls.

p Enumerator.produce(1) { |x| raise StopIteration if x > 3; x + 1 }.to_a

p Enumerator.produce([0, 1]) { |a, b| raise StopIteration if b > 20; [b, a + b] }
     .map(&:first).to_a

e = Enumerator.produce(1) { |x| x + 1 }
p [e.next, e.next]

# `loop` already swallows it, from an enumerator that ran out.
src = [1, 2].each
n = 0
loop { src.next; n += 1 }
p n
__END__
[1, 2, 3, 4]
[0, 1, 1, 2, 3, 5, 8, 13]
[1, 2]
2
