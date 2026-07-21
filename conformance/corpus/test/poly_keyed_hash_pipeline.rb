# Heterogeneous-key Hash through the full reassign-and-store pipeline.
# Mirrors optcarrot's CPU#add_mappings(addr, peek, poke):
#   peek = @peeks[peek] ||= peek
# where peek can be either a Method or a non-Method object — the param
# slot widens to poly so the `||=` lookup result flows back into the
# same local without unboxing.

class Pipeline
  def initialize
    @peeks = {}
    @stored = []
  end
  # `item` widens to poly across the call sites below (Method,
  # IntArray) so the param is sp_RbVal — the dedup reassignment
  # `item = @peeks[item] ||= item` works without re-typing the slot.
  def add(item)
    item = @peeks[item] ||= item
    @stored.push(item)
  end
  def cache_size
    @peeks.size
  end
  def stored_count
    @stored.length
  end
end

class Source
  def self.read; 42; end
end

p = Pipeline.new
m = Source.method(:read)
arr = [1, 2, 3]
p.add(m)              # cache: {m => m}
p.add(arr)            # cache: {m => m, arr => arr}
p.add(Source.method(:read))   # eql? to m → cache stays size 2
p.add(arr)            # identity hit → size still 2
puts p.cache_size     # 2
puts p.stored_count   # 4 (every call appends)
