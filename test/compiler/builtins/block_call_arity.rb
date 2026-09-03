# The arity guard on calls the bare-call spec cannot reach: block-carrying
# calls (their accepted counts are probed separately -- `Array#fill` drops
# to 0..2 under a block), zero-arity readers, Struct member access, native
# instances, and receivers beyond the original eight.
#
# Three things this file was FILED for, all fixed. `String#bytesplice`
# accepted 2..3 where CRuby has four shapes up to five arguments -- the
# replacement's own byte window was simply missing. A Struct MEMBER
# accessor counted nothing, so `s.a(2)` answered the member and
# `s.send(:a=)` indexed an empty slice. And `1.step(nil)` read the explicit
# nil as a BOUND, so the first comparison failed on a NilClass the caller
# never wrote, where ruby's endless form runs.

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
__END__
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 2, expected 1)"]
[ArgumentError, "wrong number of arguments (given 3, expected 0..2)"]
[0, 9]
:stopped
[ArgumentError, "wrong number of arguments (given 0, expected 1)"]
[ArgumentError, "wrong number of arguments (given 0, expected 2..5)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 3, expected 1..2)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1+)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 1, expected 0)"]
[ArgumentError, "wrong number of arguments (given 2, expected 0..1)"]
[ArgumentError, "wrong number of arguments (given 0, expected 1)"]
[ArgumentError, "wrong number of arguments (given 3, expected 0..2)"]
"GET"
7
[ArgumentError, "comparison of Integer with String failed"]
[ArgumentError, "comparison of Integer with String failed"]
:unbounded
