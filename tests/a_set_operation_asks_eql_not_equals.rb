# Ruby splits its container rows in two, and the split is observable.
#
#   A SEARCH asks `==`  -- `include?`, `index`, `rindex`, `count`,
#                          `delete`, `assoc`, `rassoc`, `Hash#value?`.
#   A SET OP asks `eql?` -- `-`, `&`, `|`, `intersection`, `difference`,
#                          `union`, `intersect?`, `uniq`, `Hash#slice`,
#                          `Hash#except`, `Array#eql?`, `Struct#eql?`.
#
# `1.0` against `1` separates the two: they are `==` but not `eql?`. Zeo
# drove every row with `==`, so a set operation answered the search's
# answer -- `[1.0] - [1]` came back EMPTY where ruby keeps the Float. That
# is a silently wrong answer on an ordinary mixed-numeric list.
#
# The same one rule covers a user class too: an object defining only `==`
# (no `eql?`/`hash`) is its own key, so a set operation keeps both copies
# while a search still finds either.
def show(name) = (puts "#{name}\t#{(yield).inspect}" rescue puts "#{name}\t#{$!.class}")

# --- The searches: `==`, so a Float finds an Integer ---------------------
show("include?")    { [1.0].include?(1) }
show("index")       { [1.0].index(1) }
show("rindex")      { [1.0].rindex(1) }
show("count")       { [1.0].count(1) }
show("delete")      { [1.0].delete(1) }
show("assoc")       { [[1.0, :x]].assoc(1) }
show("rassoc")      { [[:x, 1.0]].rassoc(1) }
show("ary ==")      { [1.0] == [1] }
show("hash value?") { { a: 1.0 }.value?(1) }
show("hash key")    { { a: 1.0 }.key(1) }
show("hash ==")     { { a: 1.0 } == { a: 1 } }
show("hash <=")     { { a: 1.0 } <= { a: 1 } }
show("enum count")  { [1.0].each_entry.count(1) }
show("enum index")  { [1.0].each_entry.find_index(1) }
show("enum incl?")  { [1.0].each_entry.include?(1) }

# --- The set operations: `eql?`, so it does not -------------------------
show("-")            { [1.0] - [1] }
show("&")            { [1.0] & [1] }
show("|")            { [1.0] | [1] }
show("intersection") { [1.0].intersection([1]) }
show("difference")   { [1.0].difference([1]) }
show("union")        { [1.0].union([1]) }
show("intersect?")   { [1.0].intersect?([1]) }
show("uniq")         { [1.0, 1].uniq }
show("enum uniq")    { [1.0, 1].each_entry.uniq }
show("ary eql?")     { [1.0].eql?([1]) }
show("hash eql?")    { { a: 1.0 }.eql?({ a: 1 }) }
show("hash slice")   { { 1.0 => :x }.slice(1) }
show("hash except")  { { 1.0 => :x }.except(1) }

# `slice` walks the ARGUMENTS and `except` walks the receiver, so the two
# do not answer the same order for the same pair of keys.
show("slice order")  { { a: 1, b: 2 }.slice(:b, :a) }
show("except order") { { a: 1, b: 2 }.except(:zz) }
show("slice absent") { { a: 1 }.slice(:zz) }

S = Struct.new(:x)
show("struct ==")    { S.new(1.0) == S.new(1) }
show("struct eql?")  { S.new(1.0).eql?(S.new(1)) }

# --- A user class defining only `==` ------------------------------------
class OnlyEq
  def initialize(n) = @n = n
  def ==(o) = o.is_a?(OnlyEq) && o.n == @n
  protected def n = @n
  def inspect = "OnlyEq(#{@n})"
end
a, b = OnlyEq.new(1), OnlyEq.new(1)
show("user include?") { [a].include?(b) }
show("user count")    { [a].count(b) }
show("user -")        { [a] - [b] }
show("user &")        { [a] & [b] }
show("user |")        { [a] | [b] }
show("user uniq")     { [a, b].uniq.size }

# --- A user class defining `hash` and `eql?` the ordinary way -----------
class Both
  def initialize(n) = @n = n
  def hash = 1                     # deliberately colliding
  def eql?(o) = o.is_a?(Both) && o.n == @n
  def ==(o) = eql?(o)
  protected def n = @n
  def inspect = "Both(#{@n})"
end
c, d = Both.new(1), Both.new(2)
show("both uniq")   { [c, d].uniq.size }
show("both hash")   { { c => 1, d => 2 }.size }
show("both -")      { [c, d] - [c] }
show("both &")      { [c, d] & [c] }
show("both |")      { [c] | [d] }
show("both slice")  { { c => 1, d => 2 }.slice(Both.new(1)) }
show("both except") { { c => 1, d => 2 }.except(Both.new(1)) }
