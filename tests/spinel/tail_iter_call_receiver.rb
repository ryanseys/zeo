# An iterator answers its receiver, and at tail position that value is the
# method's return. The statement form produces it by re-READING the receiver,
# so a receiver that is itself a call used to fall through to nil.
def keys_of(h) = h.keys.each { |a| a }
def values_of(h) = h.values.each { |a| a }
def pairs_of(h) = h.to_a.each { |a| a }
def sorted(a) = a.sort.each { |x| x }
def duped(a) = a.dup.each { |x| x }
def mapped(a) = a.map { |x| x }.each { |x| x }
def backwards(a) = a.sort.reverse_each { |x| x }

p keys_of({ "y" => 2 })
p values_of({ "y" => 2 })
p pairs_of({ "y" => 2 })
p sorted(["b", "a"])
p duped(["y"])
p mapped(["y"])
p backwards(["b", "a"])

# a plain read still returns the receiver itself, and a non-tail loop still
# leaves the following value alone
def plain(a) = a.each { |x| x }
def after(a)
  a.each { |x| x }
  1
end
p plain(["y"])
p after(["y"])

# the shape it turned up in: nested blocks under a module method
module M
  Only = Struct.new(:v)
  def self.walk(scope)
    scope.keys.each { |a| [1].each { |b| Only.new(a) } }
  end
end
p M.walk({ "y" => 2 })
