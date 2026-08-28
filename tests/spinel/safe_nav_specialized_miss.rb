# A miss on a specialized Hash or Array answers the ELEMENT type's own C nil --
# a NULL string, SP_INT_NIL -- not a poly nil, so a `&.` fused onto the lookup
# used to take the result type's zero and answer `false` where CRuby answers
# nil. And a `&.` whose callee renders a raw C bool (the predicates that take an
# argument, `nil?`) has to box that answer to stand beside the nil arm.

h = { "a" => "abc" }
p h["zz"]
p h["zz"]&.empty?
p h["zz"]&.start_with?("a")
p h["zz"]&.end_with?("c")
p h["zz"]&.include?("a")
p h["zz"]&.nil?
p h["zz"]&.eql?("abc")
p h["zz"]&.to_s
p h["zz"]&.size
p h["zz"]&.bytesize
p h["zz"]&.upcase
p h["a"]&.empty?
p h["a"]&.start_with?("a")
p h["a"]&.nil?

g = { "a" => 1 }
p g["zz"]&.positive?
p g["zz"]&.zero?
p g["a"]&.positive?
p g["zz"].nil?
p g["zz"] || 9

f = { "a" => 1.5 }
p f["zz"]&.positive?
p f["a"]&.positive?

a = ["abc"]
p a[5]&.empty?
p a[5]&.start_with?("a")
p a[5]&.size
p a[5]&.upcase
p a[0]&.empty?

# a nil in the literal forces the poly representation -- the control that was
# already right
m = { "a" => "abc", "b" => nil }
p m["zz"]&.empty?
p({ "a" => "abc" }.fetch("zz", nil)&.empty?)

# a poly receiver: the value arm renders a raw C bool for every one of these
def pred(v) = v&.start_with?("a")
def tail(v) = v&.end_with?("c")
def isnil(v) = v&.nil?
def has(v) = v&.include?("a")
def blank(v) = v&.empty?
def same(v) = v&.eql?("abc")
def kind(v) = v&.instance_of?(String)
def bytes(v) = v&.bytesize
def as_i(v) = v&.to_i
def as_f(v) = v&.to_f

["abc", nil].each do |v|
  p pred(v)
  p tail(v)
  p isnil(v)
  p has(v)
  p blank(v)
  p same(v)
  p kind(v)
  p bytes(v)
end

[3, nil].each do |v|
  p as_i(v)
  p as_f(v)
end

# an object receiver keeps its own nil, and a nilable ivar reaching a predicate
class Box
  attr_reader :text
  def initialize(t)
    @text = t
  end
end

def peek(b) = b.text&.empty?
p peek(Box.new("abc"))
p peek(Box.new(nil))

# an object receiver that may be nil: the guard's nil arm is a real nil, not
# the predicate's false
class Node
  def initialize(a)
    @a = a
    @kids = [1, 2]
  end

  def leaf? = @kids.empty?
  def name = @a
end

def find(x) = x > 0 ? Node.new("n") : nil
p find(1)&.leaf?
p find(-1)&.leaf?
p find(-1)&.name

# a by-value struct is never nil, so the call still dispatches -- but its
# answer has to be boxed to sit in the poly slot the widening asks for
class Flag
  def initialize(b) = @b = b
  def on? = @b
end

def flagged(x) = Flag.new(x)
p flagged(true)&.on?
p flagged(false)&.on?

# The conversions CRuby answers FOR nil: a miss handed to one of them is nil,
# and nil answers to_i as 0 and to_f as 0.0 rather than passing the sentinel on
n = { "a" => 1 }
p n["zz"].to_i
p n["zz"].to_f
p n["zz"].to_s
p n["zz"].inspect
p n["zz"].nil?
p n["a"].to_i
p n["a"].to_f
p 5.to_i
p 5.to_f
p([1][5].to_i)
p([1][0].to_i)

# and the predicates CRuby REFUSES on nil: a miss answered false where the
# program should have seen a NoMethodError
def try
  yield
rescue NoMethodError => e
  e.class
end

p try { n["zz"].positive? }
p try { n["zz"].zero? }
p try { n["zz"].even? }
p try { n["a"].positive? }
p try { n["a"].even? }
p 4.even?
p 4.positive?
p 0.zero?
p(-1.negative?)
p try { { "a" => "abc" }["zz"].empty? }
