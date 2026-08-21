# GAP -- imported from the spinel corpus at fa06b601.
#
# Object's universal protocol -- `===`, `==`, `!=`, `equal?`, `eql?`,
# `frozen?`, `freeze` -- answers differently on several native handle and
# value kinds.
#
# Three rows of the table diverge, and they are not one rule: an identity
# comparison that should be true is false, and two that should be false are
# true. Heap handles compare by IDENTITY in CRuby and carry their own frozen
# bit; the kinds here are the ones zeo represents as something other than an
# ordinary object, which is where the two models meet.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# Object's universal protocol -- ===, ==, !=, equal?, eql?, frozen?, freeze --
# on the native handle and value kinds that used to be refused at compile
# time. Heap handles compare by identity and carry the frozen bit; Random,
# OpenStruct, Exception, File::Stat, Time and Process::Tms compare
# structurally under == and ===; a poly operand is unwrapped in place; a
# case/when answers the way an explicit === does. The by-value kinds (Time,
# Tms, a String range) answer no equal? and no frozen? (bar the Range, always
# frozen). Expected output is CRuby's.
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
