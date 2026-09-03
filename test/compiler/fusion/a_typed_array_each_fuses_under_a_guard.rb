# `arr.each { }` on a statically-`Array` receiver splices the block body
# into a native loop. Unlike the literal `n.times` shape, the receiver's
# class here is a compile-time BELIEF, so the site asks TWO runtime
# questions before it may splice, and either can say no:
#
#   - is the value really an Array (a local analyze typed as one can hold
#     anything by the time the loop runs)
#   - may a splice stand in for `Array#each` (a reopen, a per-object
#     singleton, or a box makes real dispatch the only correct answer)
#
# with an ordinary block send on the other arm. Everything below has to
# behave the same whichever arm runs, which is what this file checks.
a = [1,2,3]
r = a.each { |x| print x }
puts
p r.equal?(a)
# break, next, redo-free control
p(a.each { |x| break x * 10 if x == 2 })
s = 0
a.each { |x| next if x == 2; s += x }
p s
# growth while walking: CRuby re-reads the length
b = [1]
n = 0
b.each { |x| n += 1; b << x + 1 if b.size < 4 }
p [b, n]
# a block that escapes captures a fresh binding per iteration
procs = []
a.each { |x| procs << -> { x } }
p procs.map(&:call)
# an empty array
p([].each { |x| raise "never" })
# NOT an array at run time, though analyze may believe otherwise
c = [1,2]
c = "ab" if ARGV[0]
p c.each { |x| x }
__END__
123
true
20
4
[[1, 2, 3, 4], 4]
[1, 2, 3]
[]
[1, 2]
