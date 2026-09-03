# `value.extend(Mod)` on a bare heap value -- an Array, String or Hash with no
# object wrapper. optparse's last line is `ARGV.extend(OptionParser::Arguable)`,
# so every program that reaches rubygems' option parsing runs it.
module Loud
  def shout = "#{inspect}!"
  def one_more = size + 1
end

a = [1, 2]
p a.extend(Loud).equal?(a)
p a.shout, a.one_more
# ...and only THAT value gets it.
p [3].respond_to?(:shout), a.respond_to?(:shout)

s = +"hi"
s.extend(Loud)
p s.shout, s.one_more

h = { a: 1 }
h.extend(Loud)
p h.one_more

# A second module layers on top, and an own singleton method still wins.
module Louder
  def shout = "REALLY LOUD"
end
b = [9]
b.extend(Loud)
def b.one_more = :mine
b.extend(Louder)
p b.shout, b.one_more

# A native module works the same way.
c = [3, 1, 2]
c.extend(Enumerable)
p c.sort

# Immediates have no singleton storage; nil and true do.
[1, 1.5, :sym].each do |v|
  v.extend(Loud)
rescue TypeError => e
  p [v.class, e.message]
end
p nil.extend(Loud).nil?
p true.extend(Loud)
__END__
true
"[1, 2]!"
3
false
true
"\"hi\"!"
3
2
"REALLY LOUD"
:mine
[1, 2, 3]
[Integer, "can't define singleton"]
[Float, "can't define singleton"]
[Symbol, "can't define singleton"]
true
true
