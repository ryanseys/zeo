# A poly receiver's dispatch emits the user class's arm AND the builtin arms
# into ONE C temp, so the call's type has to be one both can hold. Some arms
# asked that question and the rest did not, which is why the answer depended on
# which name you picked: `Chain#join` answering a Chain came back through the
# builtin's `const char *` as `.v.s` and printed "", `def to_i = "seven"` did
# not compile, and `def empty? = 0` came back as false -- 0 is truthy in Ruby.
#
# The question is now asked from both sides: on the way out of the poly section
# (does a user return disagree with the type this arm chose) and at the user
# dispatch (does the builtin surface disagree with what the user arms agreed
# on). This program walks the names the report enumerated, so a new arm that
# pins a concrete type has to survive them.

class B_join
  def join(*p) = "seven"
end

def p_join(v)
  v.join("b")
end

p p_join(B_join.new)
p p_join(["x", "y"])

class B_to_i
  def to_i = "seven"
end

def p_to_i(v)
  v.to_i
end

p p_to_i(B_to_i.new)
p p_to_i("12")

class B_to_f
  def to_f = 7
end

def p_to_f(v)
  v.to_f
end

p p_to_f(B_to_f.new)
p p_to_f("1.5")

class B_empty
  def empty? = 0
end

def p_empty(v)
  v.empty?
end

p p_empty(B_empty.new)
p p_empty([])

class B_index
  def index(x) = "seven"
end

def p_index(v)
  v.index("a")
end

p p_index(B_index.new)
p p_index(["a", "b"])

class B_rindex
  def rindex(x) = "seven"
end

def p_rindex(v)
  v.rindex("a")
end

p p_rindex(B_rindex.new)
p p_rindex(["a", "b"])

class B_findindex
  def find_index(x) = "seven"
end

def p_findindex(v)
  v.find_index("a")
end

p p_findindex(B_findindex.new)
p p_findindex(["a", "b"])

class B_pack
  def pack(x) = "seven"
end

def p_pack(v)
  v.pack("C*")
end

p p_pack(B_pack.new)
p p_pack([65, 66])

class B_size
  def size = "seven"
end

def p_size(v)
  v.size
end

p p_size(B_size.new)
p p_size([1, 2])

class B_length
  def length = "seven"
end

def p_length(v)
  v.length
end

p p_length(B_length.new)
p p_length([1, 2])

class B_includep
  def include?(x) = 0
end

def p_includep(v)
  v.include?(1)
end

p p_includep(B_includep.new)
p p_includep([1])

class B_memberp
  def member?(x) = 0
end

def p_memberp(v)
  v.member?(1)
end

p p_memberp(B_memberp.new)
p p_memberp([1])

# the reproduction: a method that answers an object of its own class, reached
# through a widened receiver, with the receiver's own arm running (@asked is 1)
class Chain
  def initialize(text)
    @text = text
    @asked = 0
  end

  attr_reader :asked

  def to_s = @text

  def join(*parts)
    @asked += 1
    parts.reduce(self) { |acc, part| Chain.new("#{acc}/#{part}") }
  end
end

def joined(value)
  value.join("b")
end

chain = Chain.new("/tmp")
p "#{joined(chain)}"
p chain.asked
p joined(["x", "y"])

# a user arm that AGREES keeps the concrete type: this is what must not widen,
# or every poly.size boxes a count the whole program reads as a number
class Counted
  def size = 3
  def length = 4
end

def sized(v) = v.size
def lengthed(v) = v.length
p sized(Counted.new)
p sized([1, 2])
p lengthed(Counted.new)
p lengthed("abcde")
