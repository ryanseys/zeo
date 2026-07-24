# Hash with Method-typed keys — the optcarrot @peeks/@pokes dedup
# pattern. Two `obj.method(:foo)` calls produce distinct Method
# instances but eql? as keys (same bound receiver + fn_ptr), so the
# `@cache[k] ||= k` cache stores only one entry for them.

class Cache
  def initialize
    @h = {}
  end
  def add(k)
    @h[k] ||= k
  end
  def has?(k)
    !@h[k].nil?
  end
  def size
    @h.size
  end
end

class M
  def self.foo; end
  def self.bar; end
end

c = Cache.new
m1 = M.method(:foo)
c.add(m1)
puts c.has?(m1)             # true (same instance round-trips)
puts c.has?(M.method(:bar)) # false (different fn_ptr)

# Two distinct Method instances for the same (receiver, fn_ptr)
# hash and eql? identically, so the cache stays at size 1.
c.add(M.method(:foo))
puts c.size                 # 1
puts c.has?(M.method(:foo)) # true via eql? (m1 returned for the
                            # query Method instance)
