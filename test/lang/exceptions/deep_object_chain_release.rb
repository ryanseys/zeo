# Releasing a long chain of objects must not cost one stack frame per link.
class Link
  attr_accessor :nxt
  def initialize(n)
    @nxt = n
    @tag = "link"
  end
end

def chain(len)
  head = nil
  len.times { head = Link.new(head) }
  head
end

head = chain(200_000)
n = 0
cur = head
while cur
  n += 1
  cur = cur.nxt
end
p n

# Dropping the only reference releases all 200,000 at once.
head = nil
GC.start
p chain(200_000).nxt.nxt.nil?

# A chain through an ivar that a runtime path invented releases the same way.
deep = Link.new(nil)
200_000.times do
  outer = Link.new(nil)
  outer.instance_variable_set(:@inner, deep)
  deep = outer
end
p deep.instance_variable_get(:@inner).class
deep = nil
GC.start
p "released"
__END__
200000
false
Link
"released"
