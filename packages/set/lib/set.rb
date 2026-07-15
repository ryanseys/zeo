# Pure-Ruby Set for spinel (Phase 14.4) -- the flagship validation that a
# stdlib-shaped package can be written in plain Ruby and compiled by the
# same whole-program pipeline as user code: it exercises `include
# Enumerable` (the RUST-implemented builtin module in
# spinel_rt::enumerable -- map/select/find/... all drive this class's own
# #each), blocks/&blk forwarding through nested escaping Procs,
# operator-method definitions, and the dynamic-dispatch layer (a Set's
# state is a Poly-typed @hash ivar; every operation on it routes through
# spinel_rt::send_value's builtin table). Semantics follow core Ruby's Set
# (insertion-ordered; #== is membership-based) for the surface implemented
# here.
class Set
  include Enumerable

  def initialize(items = nil)
    @hash = {}
    unless items.nil?
      items.each { |x| @hash[x] = true }
    end
  end

  def add(item)
    @hash[item] = true
    self
  end

  def <<(item)
    add(item)
  end

  def delete(item)
    @hash.delete(item)
    self
  end

  # Overrides Enumerable#include? with the O(1) hash lookup.
  def include?(item)
    @hash.key?(item)
  end

  def member?(item)
    include?(item)
  end

  def size
    @hash.size
  end

  def length
    size
  end

  def empty?
    @hash.empty?
  end

  def each(&blk)
    @hash.keys.each { |k| blk.call(k) }
    self
  end

  # Set defines its own to_a (real Ruby's Set does too: the hash's keys,
  # in insertion order) rather than relying on Enumerable#to_a.
  def to_a
    @hash.keys
  end

  def |(other)
    result = Set.new(to_a)
    other.each { |x| result.add(x) }
    result
  end

  def union(other)
    self | other
  end

  def &(other)
    result = Set.new
    each { |x| result.add(x) if other.include?(x) }
    result
  end

  def intersection(other)
    self & other
  end

  def -(other)
    result = Set.new
    each { |x| result.add(x) unless other.include?(x) }
    result
  end

  def difference(other)
    self - other
  end

  def ^(other)
    (self | other) - (self & other)
  end

  def ==(other)
    return false unless other.is_a?(Set)
    size == other.size && subset?(other)
  end

  def subset?(other)
    ok = true
    each { |x| ok = false unless other.include?(x) }
    ok
  end

  def superset?(other)
    other.subset?(self)
  end
end
