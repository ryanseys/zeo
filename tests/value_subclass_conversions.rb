class S < String; end
class A < Array; end
class H < Hash; end
s = S.new("x"); a = A.new([1]); h = H.new

# Identity CONVERSIONS demote to the base class...
puts s.to_s.class
puts s.to_str.class
puts a.to_a.class
puts h.to_h.class

# ...but their near-twins return self, keeping the subclass.
puts a.to_ary.class
puts h.to_hash.class

# Self-returning mutators still re-wrap.
puts a.dup.push(2).class
puts s.dup.concat("y").class

# A derived value stays a plain base-class object.
puts s.upcase.class
puts a.map { |v| v }.class

# The crash this fixed: a subclass whose <=> reads to_s never reached a
# plain String, so the user method re-dispatched until the stack died.
class Backwards < String
  def <=>(o); o.to_s <=> to_s; end
end
p [Backwards.new("aaa"), Backwards.new("bbb")].sort.map(&:to_s)
