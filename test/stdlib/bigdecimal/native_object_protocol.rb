# Object's universal protocol -- `===`, `==`, `!=`, `equal?`, `eql?`,
# `frozen?`, `freeze` -- over the native handle and value kinds, promoted
# from `tests/gaps/` on 2026-08-21.
#
# The rule is not one rule. A heap HANDLE (Fiber, Mutex, Queue, Thread, IO,
# Dir, MatchData) compares by IDENTITY and carries a frozen bit of its own.
# A by-VALUE kind (Time, `Process::Tms`, a Struct-backed row) compares
# structurally under `==` and `===`, answers no `equal?`, and -- for most of
# them -- no `frozen?` either. A Range is the exception that is always
# frozen.
#
# `eql?` follows NEITHER, which is why the last block pins it as a table.
# `Set`, `Time`, `Date`, `Pathname` and `BigDecimal` answer it by value;
# `FFI::Pointer` and an `Exception` answer `==` by value and `eql?` by
# IDENTITY. Nothing zeo can see predicts the split, so
# `Kernel#eql?`'s `owns_value_eql` allowlist is measured against the oracle
# and this table is what keeps it honest.
#
# Expected output is CRuby's.
require "socket"
require "ostruct"

f = Fiber.new { }
g = Fiber.new { }
p [f === f, f === g, f == g, f != g, f.equal?(f), f.eql?(g), f.frozen?, f.freeze.frozen?]

m = Mutex.new
cv = ConditionVariable.new
q = Queue.new
th = Thread.new { }
th.join
p [m === m, m === cv, m == 1, m != "x", cv === cv, cv.equal?(cv), cv.eql?(ConditionVariable.new), cv.frozen?]
p [q === q, q == q, q.equal?(Queue.new), th === th, th == th, th.eql?(th), q.frozen?]

r = Random.new(7)
r2 = Random.new(7)
r3 = Random.new(8)
p [r === r2, r == r2, r != r3, r == r3, r.equal?(r2), r.eql?(r2), r.eql?(r), r.frozen?, r.freeze.frozen?]

os = OpenStruct.new(a: 1)
os2 = OpenStruct.new(a: 1)
boxed = [os, 1, OpenStruct.new(a: 1.0)]
p [os === os2, os == os2, os != os2, os.equal?(os2), os.equal?(os), os.eql?(os2), os == boxed[0], os.eql?(boxed[1])]
# eql? is the member table's: class-strict per member
p [os == boxed[2], os.eql?(boxed[2]), os != boxed[0], os === boxed[0], os.equal?(boxed[0]), os.eql?(OpenStruct.new(a: 1.0))]

t = Time.at(5)
t2 = Time.at(5)
t3 = Time.at(6)
boxed_t = [t, 1]
p [t === t2, t === t3, t == t2, t != t3, t.eql?(t2), t.eql?(t3), t.eql?(boxed_t[0]), t.eql?(boxed_t[1]), t == boxed_t[0], t != boxed_t[1]]

tms = Process.times
tms2 = tms.dup
boxed_tms = [tms, 1]
p [tms === tms2, tms == tms2, tms != tms2, tms.eql?(tms2), tms == 1, tms == boxed_tms[0], tms.eql?(boxed_tms[0]), tms != boxed_tms[1]]

sr = ("a".."c")
sr2 = ("a".."c")
sr3 = ("a".."d")
p [sr == sr2, sr != sr3, sr != sr2, sr.eql?(sr3), sr.frozen?]

d = Dir.new(".")
p [d === d, d == d, d != d, d.equal?(d), d.eql?(Dir.new(".")), d == 3, d.frozen?, d.freeze.frozen?]

ai = Addrinfo.tcp("127.0.0.1", 80)
ai2 = Addrinfo.tcp("127.0.0.1", 80)
p [ai === ai, ai === ai2, ai == ai2, ai != ai2, ai.equal?(ai), ai.eql?(ai2), ai.eql?("x"), ai.frozen?]

io = File.open("/dev/null")
st = File.stat(".")
p [io === io, io == File.open("/dev/null"), io != io, io.equal?(io), io.frozen?, io.freeze.frozen?]
st2 = File.stat(".")
root = File.stat("/")
p [st === st, st.equal?(st), st.frozen?, st.freeze.frozen?]
# File::Stat is Comparable over the modification time: two stats of one file are ==
p [st == st2, st === st2, st != st2, st.eql?(st2), st.equal?(st2), st == root, st === root, st == io, io == st]

me = 1.method(:+)
en = [1].each
p [me.frozen?, me.freeze.frozen?, en === en, en.equal?([1].each), en.frozen?]

e = RuntimeError.new("x")
e2 = RuntimeError.new("x")
e3 = RuntimeError.new("y")
boxed_e = [e, 1]
p [e === e, e === e2, e === e3, e == e2, e != e3, e.equal?(e2), e === boxed_e[0], e.equal?(boxed_e[1]), e.frozen?, e.freeze.frozen?]
p [e == boxed_e[0], e != boxed_e[0], e.eql?(boxed_e[0]), e == boxed_e[1]]

md = /a/.match("a")
md2 = /a/.match("a")
boxed_m = [md, 1]
p [md === md, md === md2, md == md2, md.equal?(md), md.equal?(md2), md === boxed_m[0], md.equal?(boxed_m[0]), md.equal?(boxed_m[1])]
p [md.frozen?, md.freeze.frozen?]

# a Range, Array or Bignum against an operand of another family
big = 10**20
p [(1..2) == [1, 2], (1..2) != [1, 2], (1..2) == (1..2), (1.0..2.0) == "x", ("a".."b") == [1], (1.0..2.0) != (1.0..2.0), (1.0..2.0) != (1.0..3.0)]
p [[1] == 1, [1] != 1, [1] == [1.0], [1] != [1.0], [1.0] == [1], [1, 2].eql?([1.0, 2.0])]
p [big.equal?("x"), big.eql?("x"), big == "x", big != :sym, big == big]

case f
when g then p :other
when f then p :self
end

# case/when answers the way an explicit === does
case e2
when e3 then p :other_message
when e then p :same_message
end
case os2
when os then p :same_members
else p :no
end
case Time.at(5)
when t then p :same_instant
else p :no
end
case md2
when md then p :same_match
else p :no
end
case 5
when f then p :no
else p :not_a_fiber
end

# the standard streams are static storage: freeze flips a flag of their own
p $stderr.frozen?
$stderr.freeze
p $stderr.frozen?

# a nil handle slot answers nil's frozen?
class Holder
  attr_reader :io
  def open!
    @io = File.open("/dev/null")
  end
end
h = Holder.new
p [h.io, h.io.frozen?]
h.open!
p h.io.frozen?

# the receiver is evaluated once, first, whatever the operand holds
def noisy
  puts "receiver"
  Dir.new(".")
end
mixed = [1, "s"]
p(noisy == mixed[0])
p(noisy === mixed[1])
p(noisy != mixed[0])

# The `eql?` table. A class here either owns a value `eql?` or inherits
# Object's identity one; measured, because the split follows no rule.
require "set"
require "date"
require "pathname"
require "bigdecimal"
require "ffi"
def eql_row(label, a, b)
  print "#{label}: "
  p [a == b, a.eql?(b), a.equal?(b)]
end
eql_row("Set", Set[1, 2], Set[2, 1])
eql_row("Time", Time.at(5), Time.at(5))
eql_row("Date", Date.new(2020, 1, 1), Date.new(2020, 1, 1))
eql_row("DateTime", DateTime.new(2020, 1, 1), DateTime.new(2020, 1, 1))
eql_row("Pathname", Pathname.new("/a"), Pathname.new("/a"))
eql_row("BigDecimal", BigDecimal("1.5"), BigDecimal("1.5"))
eql_row("Encoding", Encoding::UTF_8, Encoding::UTF_8)
eql_row("Range", (1..2), (1..2))
eql_row("ArithSeq", 1.step(5, 2), 1.step(5, 2))
eql_row("Rational", Rational(1, 2), Rational(1, 2))
eql_row("Regexp", /a/, /a/)
eql_row("Pointer", FFI::Pointer.new(8), FFI::Pointer.new(8))
eql_row("Exception", ArgumentError.new("x"), ArgumentError.new("x"))
eql_row("Lazy", [1].lazy, [1].lazy)
eql_row("Enumerator", [1].each, [1].each)
tms = Process.times
eql_row("Tms", tms, tms.dup)
__END__
[true, false, false, true, true, false, false, true]
[true, false, false, true, true, true, false, false]
[true, true, false, true, true, true, false]
[true, true, true, false, false, false, true, false, true]
[true, true, false, false, true, true, true, false]
[true, false, false, true, true, false]
[true, false, true, true, true, false, true, false, true, true]
[true, true, false, true, false, true, true, true]
[true, true, false, false, true]
[true, true, false, true, false, false, false, true]
[true, false, false, true, true, false, false, false]
[true, false, false, true, false, true]
[true, true, false, true]
[true, true, false, false, false, false, false, false, false]
[false, true, true, false, false]
[true, true, false, true, true, false, true, false, false, true]
[true, false, true, false]
[true, true, true, true, false, true, true, false]
[false, true]
[false, true, true, false, false, false, true]
[false, true, true, false, true, false]
[false, false, false, true, true]
:self
:same_message
:same_members
:same_instant
:same_match
:not_a_fiber
false
true
[nil, true]
false
receiver
false
receiver
false
receiver
true
Set: [true, true, false]
Time: [true, true, false]
Date: [true, true, false]
DateTime: [true, true, false]
Pathname: [true, true, false]
BigDecimal: [true, true, false]
Encoding: [true, true, true]
Range: [true, true, false]
ArithSeq: [true, true, false]
Rational: [true, true, false]
Regexp: [true, true, false]
Pointer: [true, false, false]
Exception: [true, false, false]
Lazy: [false, false, false]
Enumerator: [false, false, false]
Tms: [true, true, false]
