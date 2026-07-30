# A shared-mutable string (#3227) pushed into a typed Array[String].
#
# A string that is both aliased and mutated in place reads as its sp_String*
# handle. A typed array's element slot is a plain const char*, so the handle
# has to be spent at the boundary. Pushed raw it went in as a struct pointer,
# which the C compiler only warned about -- the build linked and the element
# read back as garbage. No exception, no crash, wrong bytes.
#
# The local-array case did not show it: a local widened to a poly array, where
# the value is boxed. The ivar kept its typed array, which is why the report is
# specifically about an ivar.

class Holder
  attr_accessor :items, :map

  def initialize
    @items = [""]      # type seed; a bare [] infers an IntArray
    @items.pop
    @map = { "seed" => "" }
    @map.delete("seed")
  end
end

def built(x)
  s = +""
  s << "v"
  s << x
  s
end

h = Holder.new
h.items << built("1")
puts "after <<:   " + h.items[0]
h.items.push(built("2"))
h.items.unshift(built("0"))
h.items.insert(1, built("x"))
p h.items
h.items.each { |e| puts "in each:    " + e }

h.map["k"] = built("m")
p h.map["k"]

h.items.concat([built("c")])
p h.items.last
p h.items.join(",")
p h.items.length

# the reporter's shape, verbatim in spirit
s = +""
s << "b"
s << "=2"
h2 = Holder.new
h2.items << s
puts "reporter:   " + h2.items[0]
