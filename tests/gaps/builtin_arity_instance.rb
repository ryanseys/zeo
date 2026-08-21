# GAP -- imported from the spinel corpus at fa06b601.
#
# zeo PANICS rather than raising. `Enumerable#first` with two arguments hits
#
#     panicked at crates/zeo-rt/src/builtins/enumerable.rs:
#     Enumerable#first takes at most one argument
#
# where CRuby raises `ArgumentError: wrong number of arguments (given 2,
# expected 0..1)`. A panic cannot unwind out of `extern "C"`, so this ends
# the process.
#
# The rule it breaks is the one the arity work already states: a builtin row
# checks its own count and raises CRuby's ArgumentError. An `expect` in the
# body is the wrong instrument for an argument the CALLER controls.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# A builtin instance method called with a count CRuby rejects raises CRuby's
# ArgumentError (with its message) instead of reporting NoMethodError, silently
# dropping the extras, or crashing the compiler reading a missing operand.
def check
  yield
rescue => e
  p [e.class, e.message]
end

# missing required argument (compile-time SIGSEGV before the arity guard)
check { (1..3).cover? }
check { (1..3).include? }
# an operator spelled as a method call with its operand missing
check { :a.<=> }
check { 1.+() }
# too few for an exact multi-argument method
s = +"x"
check { s.insert(1) }
h = {a: 1}
check { h.store(1) }
# too many for a zero-argument method
check { :a.to_s(9) }
check { "x".succ(1) }
# too many for a bounded optional range
check { "x".chomp("a", "b") }
check { [1, 2].first(1, 2) }
check { (1..2).first(1, 2) }
# variadic minimum
a = [1]
check { a.insert }
# the valid shapes still answer
p (1..2).first
p "x".chomp
p [1, 2].first(1)
# a block changes several counts: the bare-call spec must not fire
p [1].fill { 9 }
