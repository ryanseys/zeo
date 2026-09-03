# Refinements are a compile-time decision in a program: a `refine` mints a
# holder the compiler registers, and a `using` rewrites the call sites in
# its lexical range. A snippet has neither -- its module is a run-time
# constant and what it refines lives in the running program's registry --
# so both go through the run time: `Module#refine` mints and marks the
# holder, and a `using` fills an activation slot every call site it covers
# reads.

module Outer
  refine Integer do
    def trip = self * 3
  end
end

MINT = "module Minted; refine String do; def shout = upcase + '!'; end; end"
eval(MINT)
p Minted.refinements.size
p Minted.refinements.first.class
p "hi".respond_to?(:shout)

USE = [
  "using Outer",
  "p 7.trip",
  "p 7.respond_to?(:trip)",
  "p 7.send(:trip)",
  "p 7.method(:trip).owner.to_s",
  "p [1, 2].map { |x| x.trip }",
  # A `def` written after the `using` sees it too: the slot outlives the
  # call, exactly as CRuby's lexical activation outlives the eval.
  "def tripler(n) = n.trip",
].join("\n")
eval(USE)
p tripler(5)

# ... and nothing outside the snippet's own range sees it.
begin
  3.trip
rescue NoMethodError
  puts "outside: NoMethodError"
end

# A snippet that both mints and activates.
BOTH = [
  "module Both; refine Symbol do; def twice = [self, self]; end; end",
  "using Both",
  "p :a.twice",
].join("\n")
eval(BOTH)

# `using` answers the receiver it activated on.
ACT = "using Outer"
p eval(ACT)
class Host; end
p Host.class_eval(ACT)
__END__
1
Refinement
false
21
true
21
"#<refinement:Integer@Outer>"
[3, 6]
15
outside: NoMethodError
[:a, :a]
main
Host
