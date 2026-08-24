# A `require` written inside a METHOD body loads its feature EAGERLY, before
# the program's first statement, where CRuby loads it when the method runs.
#
# `Hir::deferred_requires` documents the intended rule -- "CRuby loads such a
# file when the method runs; whole-program AOT has no runtime loader, so the
# honest answer is to leave it out and let the CALL lower to a runtime
# `Kernel#require`". The loader also records the file in `single_unit_demand`
# so it is compiled in. What is missing is the JOIN: the run-time
# `Kernel#require` does not find a unit registered under that feature name.
#
# So the shape works today only because the binary carries the embedded
# compiler, which recompiles the file from source at the call. That is what
# makes a deferred require cost 14 MB, and it is why the size predicate cannot
# discount one -- the honest fix is to register the unit under the feature the
# call names, and then a deferred require costs a unit rather than a compiler.
#
# Two divergences here, and the second is the one that blocks the size work:
# the feature is defined before the call, and `require` inside a method is the
# reason an eval-free program carries the compiler.

def lazy
  require "prettyprint"
end

p defined?(PrettyPrint)
p lazy
p defined?(PrettyPrint)
p lazy
