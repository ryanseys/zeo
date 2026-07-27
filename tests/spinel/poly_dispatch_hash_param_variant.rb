# Two classes defining the same method with a Hash parameter, called on a
# receiver whose type is the union of both. A parameter already typed as a
# concrete hash by its callers was being NARROWED from the method body's own
# writes -- str->poly down to str->str -- which converts nothing: it
# reinterprets the caller's sp_StrPolyHash * through an sp_StrStrHash *
# parameter. The two arms of the dispatch then disagreed on the variant, the
# call passed the wrong struct, and the program hung.
class Leaf
  def initialize(char) = @char = char
  def walk(table) = table[@char] = "x"
end

class Branch
  def initialize(kid) = @kid = kid
  def walk(table) = @kid.walk(table)
end

nodes = [Leaf.new("a")]
nodes << Branch.new(Leaf.new("b"))
tree = nodes.last

table = {}
tree.walk(table)
p table["b"].length
p table.size

# both arms reached from the same site, mutating the caller's hash in place
shared = {}
nodes.each { |n| n.walk(shared) }
p shared.keys.sort
p shared["a"]
p shared["b"]

# ... and read back with a key that comes from ANOTHER hash's iteration, which
# is what made the caller's `{}` and the callee's parameter name different C
# structs: the key context narrowed the literal while the callee, invisible to
# it, filled the hash with its own key type (#3386).
freqs = { "a" => 5, "b" => 9 }
tally = {}
nodes.each { |n| n.walk(tally) }
p freqs.sum { |ch, f| f * tally[ch].length }
p freqs.map { |ch, _| tally[ch] }.sort
