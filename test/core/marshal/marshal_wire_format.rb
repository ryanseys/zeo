# Marshal wire format: the `I` encoding wrapper, `/` Regexp, `c`/`m` class and
# module references, `S` Struct, and the `U`/`u` user hooks -- all byte-checked
# against CRuby and round-tripped.
def wire(x) = Marshal.dump(x).bytes.join(",")
def rt(x) = Marshal.load(Marshal.dump(x))

# A string's encoding rides an `I` wrapper: E=true (UTF-8), E=false (US-ASCII),
# encoding="<name>" (other), no wrapper for ASCII-8BIT.
puts wire("abc")
puts wire("abc".b)
puts wire("café")
puts wire("abc".encode("Shift_JIS"))
puts rt("café").encoding.name
puts rt("abc".b).encoding.name
puts rt("abc".encode("Shift_JIS")).encoding.name

# Shared strings link the second time, so a mutation is visible through both.
s = "ab"
g = rt([s, s])
g[0] << "z"
p g[1]

# Regexp: source bytes + an option byte (i=1, x=2, m=4), I-wrapped.
puts wire(/ab.c/im)
r = rt(/a\d+b/i)
puts "#{r.source} #{r.options}"

# Class / module references.
puts wire(String)
p rt(String)
p rt(Comparable)

# Struct: class symbol, member count, member/value pairs.
Point = Struct.new(:x, :y)
puts wire(Point.new(1, 2))
pt = rt(Point.new(10, "east"))
p [pt.x, pt.y]

# `U`: marshal_dump / marshal_load.
class Temp
  def initialize(c = 0) = (@celsius = c)
  attr_reader :celsius
  def marshal_dump = [@celsius]
  def marshal_load(a) = (@celsius = a[0])
end
puts wire(Temp.new(21))
p rt(Temp.new(100)).celsius

# `u`: _dump / self._load, with a binary payload.
class Version
  def initialize(n = 0) = (@n = n)
  attr_reader :n
  def _dump(depth) = [@n].pack("N")
  def self._load(s) = new(s.unpack1("N"))
end
puts wire(Version.new(258))
p rt(Version.new(65535)).n

# `C`: a subclass of a value builtin marshals the inherited body inline.
class Tag < String; end
puts wire(Tag.new("v1"))
t = rt(Tag.new("v1"))
puts "#{t.class} #{t.upcase}"
__END__
4,8,73,34,8,97,98,99,6,58,6,69,84
4,8,34,8,97,98,99
4,8,73,34,10,99,97,102,195,169,6,58,6,69,84
4,8,73,34,8,97,98,99,6,58,13,101,110,99,111,100,105,110,103,34,14,83,104,105,102,116,95,74,73,83
UTF-8
ASCII-8BIT
Shift_JIS
"abz"
4,8,73,47,9,97,98,46,99,5,6,58,6,69,70
a\d+b 1
4,8,99,11,83,116,114,105,110,103
String
Comparable
4,8,83,58,10,80,111,105,110,116,7,58,6,120,105,6,58,6,121,105,7
[10, "east"]
4,8,85,58,9,84,101,109,112,91,6,105,26
100
4,8,117,58,12,86,101,114,115,105,111,110,9,0,0,1,2
65535
4,8,73,67,58,8,84,97,103,34,7,118,49,6,58,6,69,84
Tag V1
