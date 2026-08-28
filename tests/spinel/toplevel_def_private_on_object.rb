# A top-level `def` lands on Object as a PRIVATE method: CRuby answers an
# explicit-receiver call with "private method called", never with the method.
# Spinel counted such a def as owning the name for every receiver dispatch, so
# a builtin arm stood down for it -- and with no user CLASS defining the name,
# `v.upcase` on a boxed receiver compiled to an unconditional NoMethodError.
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
