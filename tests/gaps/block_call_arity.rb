# GAP -- imported from the spinel corpus at fa06b601.
#
# Two rows in zeo's builtin ARITY table are narrower than CRuby's, and one
# call raises the wrong error entirely.
#
#   Array#fill under a block   zeo accepts 2..3, CRuby 2..5
#   Numeric#step with a block  a zero-argument call raises
#                              `comparison of Integer with NilClass failed`
#                              from inside `step`, where CRuby raises the
#                              ordinary ArgumentError before the body runs
#
# The arity ratchet (`conformance/builtin-arity.tsv`) gates the declared
# counts, so the first is a table row to correct. The second is a missing
# guard: the call reaches the body with a nil bound and fails on the
# comparison, which reports a cause the caller never wrote.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# The arity guard on calls the bare-call spec could not reach: block-carrying
# calls (their accepted counts are probed separately -- Array#fill drops to
# 0..2 under a block), zero-arity readers, Struct member access, native-class
# instances, and receivers beyond the original eight (nil, bools, Rational,
# Complex, user objects falling back to Object's universal rows).
require "stringio"

# block-carrying calls check the with-block quartet
begin
  p(3.times(1) { |i| i })
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(3.upto(4, 1) { |i| i })
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(3.step(4, 1, 2) { |i| i })
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p([1, 2].fill(0) { |i| i * 9 })
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(2.step { break :stopped })
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# methods the Method#arity dump missed
begin
  p(1.5.div)
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p("ab".bytesplice)
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# receivers beyond the original map
begin
  p(nil.to_a(1))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(true.to_s(1))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(1r.numerator(1))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(1i.real(1))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# a user object falls back to Object's universal rows
class Widget; end
w = Widget.new
begin
  p(w.frozen?(1))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(w.respond_to?(:a, false, 2))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(w.dup(1))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# attr and Struct readers are zero-arity; Struct [] / dig want arguments
class Holder
  attr_reader :size
  def initialize = @size = 5
end
begin
  p(Holder.new.size(2))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
pair = Struct.new(:a, :b).new(1, 2)
begin
  p(pair.a(2))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(pair[])
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(pair.dig)
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# native-class instances and Mutex check their probed rows
begin
  p(StringIO.new("x").eof?(1))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(Mutex.new.synchronize(1) { 2 })
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# class-method rows: constructors and renamed forms
begin
  p(Hash.new(1, 2))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(File.exist?)
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end
begin
  p(Array.new(1, 2, 3))
rescue TypeError, ArgumentError => e
  p [e.class, e.message]
end

# review-found edges: an attribute named like an Object method is the
# attribute; empty splats are zero arguments; the iterator path type-checks
class Request
  attr_accessor :method, :path
  def initialize(m, p2) = (@method = m; @path = p2)
end
p Request.new("GET", "/").method
class Pt; attr_reader :x; def initialize = @x = 7; end
p Pt.new.x(*[])
begin
  3.step("a") { |i| i }
rescue ArgumentError => e
  p [e.class, e.message]
end
begin
  5.between?("a", "b")
rescue ArgumentError => e
  p [e.class, e.message]
end
p 1.step(nil) { |i| break :unbounded }
