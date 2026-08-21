# Ruby evaluates a call's receiver before its arguments. Spinel handed both to
# one C call, where the order among the operands is unspecified: gcc picks
# right-to-left and clang picks left-to-right, so the same program printed the
# argument's trace before the receiver's on Linux and after it on macOS. The
# receiver binds to a temp in front of the call now, in place rather than at the
# statement above, so an expression inside a block body still evaluates where it
# was written.
def r_str(v); puts "R str"; v; end
def a_str(v); puts "A str"; v; end
p r_str(String.new("abc")).include?(a_str("b"))
p r_str(String.new("hello")).index(a_str("l"))

def r_arr(v); puts "R arr"; v; end
def a_arr(v); puts "A arr"; v; end
p r_arr([1, 2, 3]).include?(a_arr(2))
p r_arr([1, 2, 3]).fetch(a_arr(0))

def r_hash(v); puts "R hash"; v; end
def a_hash(v); puts "A hash"; v; end
p r_hash({"k" => 1}).key?(a_hash("k"))

def r_obj(v); puts "R obj"; v; end
def a_obj(v); puts "A obj"; v; end
class Adder
  def initialize; @seen = []; end
  def take(x); @seen.push(x); @seen.length; end
end
p r_obj(Adder.new).take(a_obj(7))

# The receiver is bound where it is written, not hoisted out of a block body:
# these operands read block parameters that only exist inside it.
p [10, 20].each.with_index.inject(0) { |t, pr| t + pr[0] + pr[1] }
p [[1, 2], [3, 4]].map { |a, b| a.to_s + b.to_s }
