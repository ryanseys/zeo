# A top-level `def` lands on Object as a PRIVATE method: CRuby answers an
# explicit-receiver call with "private method called", never with the method.
# So a top-level def must not be counted as owning the name for every
# receiver: a builtin that answers it still answers it, and `v.upcase` on a
# receiver of unknown type still reaches String#upcase.
def upcase(v) = "top-upcase:#{v}"
def chars(v) = "top-chars:#{v}"
def empty?(v) = v.nil?
def size(v) = -1

def up(v) = v.upcase
def ch(v) = v.chars
def blank(v) = v&.empty?

p up("abc")
p ch("abc")
p blank("abc")
p blank(nil)

# the top-level methods themselves still answer an implicit-receiver call
p upcase(1)
p chars(2)
p empty?(nil)
p size(3)

# a user CLASS defining the name still owns it: the dispatch keeps a builtin
# arm alongside the class arm
class Shout
  def upcase = "SHOUT"
end

def either(v) = v.upcase
p either("abc")
p either(Shout.new)
__END__
"ABC"
["a", "b", "c"]
false
nil
"top-upcase:1"
"top-chars:2"
true
-1
"ABC"
"SHOUT"
