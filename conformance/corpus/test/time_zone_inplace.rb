# Time#utc / #gmtime / #localtime mutate the receiver (#2637): sp_Time is a
# value struct, so an lvalue receiver (local, ivar) takes a write-back
# assignment. The get* flavors stay non-destructive. (Aliasing two variables to
# one Time and observing the mutation through the other is a value-type limit:
# assignment copies.)
v = Time.at(0)
v.utc
p v.utc?
w = Time.at(0)
w.gmtime
p w.utc?
x = Time.utc(2020, 1, 1)
x.localtime
p x.utc?
y = Time.at(0)
p y.utc.utc?
z = Time.at(0)
z.getutc
p z.utc?
class Holder
  def initialize; @t = Time.at(0); end
  def flip; @t.utc; end
  def utc?; @t.utc?; end
end
h = Holder.new
h.flip
p h.utc?
