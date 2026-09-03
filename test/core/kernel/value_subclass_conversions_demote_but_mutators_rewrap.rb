# A value subclass (`class S < String`) carries its payload behind a bridge
# that re-wraps a builtin's return value into the subclass when it hands back
# the SAME handle -- the no-allowlist heuristic for self-returning mutators
# (`push`/`concat`). The identity CONVERSIONS return that same handle too but
# must DEMOTE to the base class, and CRuby draws the line precisely: `to_s`/
# `to_str`/`to_a`/`to_h` demote, while `to_ary`/`to_hash` return self. Getting
# it wrong was not merely a wrong class -- a subclass whose `<=>` read
# `o.to_s <=> to_s` never reached a plain String, so the user method
# re-dispatched until the stack overflowed.

class S < String; end
class A < Array; end
class H < Hash; end
s = S.new("x"); a = A.new([1]); h = H.new
puts s.to_s.class
puts s.to_str.class
puts a.to_a.class
puts h.to_h.class
puts a.to_ary.class
puts h.to_hash.class
puts a.dup.push(2).class
puts s.dup.concat("y").class
puts s.upcase.class
puts a.map { |v| v }.class
class Backwards < String
  def <=>(o); o.to_s <=> to_s; end
end
p [Backwards.new("aaa"), Backwards.new("bbb")].sort.map(&:to_s)
__END__
String
String
Array
Hash
A
H
A
S
String
Array
["bbb", "aaa"]
