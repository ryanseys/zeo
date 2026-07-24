# Array#reduce with a non-constant Symbol operator (a |sym| block param):
# fold through a runtime operator dispatch, yielding a boxed poly.
p [:+, :-, :*].map { |sym| [10, 3].reduce(sym) }
p [:+, :*].map { |s| [2, 3, 4].reduce(10, s) }
ops = [:&, :|]
p ops.map { |o| [6, 3].reduce(o) }
p [:+, :-].map { |s| [1.0, 2.0].reduce(s) }
# literal and static-local forms are unchanged
p [2, 3, 4].reduce(:+)
p [2, 3, 4].reduce(10, :*)
