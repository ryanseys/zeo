# A Struct / Data / attr_accessor read inside a block, where the receiver is a
# poly array element: the member must win over a same-named String method, and
# #[] must reach the member table.
E = Struct.new(:ip, :ms)
[E.new("a", 10)].each { |e| p e[:ms] }
[E.new("a", 10)].each { |e| p e["ms"] }
[E.new("a", 10)].each { |e| p e[1] }
[E.new("a", 10)].each { |e| p e[0] }
[E.new("a", 10)].each { |e| p e[-1] }
[E.new("a", 10)].each { |e| p e.ms }

# a reader named after a String method
S = Struct.new(:ip, :bytes)
[S.new("a", 10)].each { |e| p e.bytes }
class K
  attr_accessor :ip, :bytes, :chars
  def initialize(a, b, ch); @ip = a; @bytes = b; @chars = ch; end
end
[K.new("a", 10, 7)].each { |e| p e.bytes }
[K.new("a", 10, 7)].each { |e| p e.chars }

# a real String in a poly slot still gets String#bytes
p ["ab"].map { |s| s.bytes }
