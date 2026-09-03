# A static regexp literal is compiled once per SITE, not once per evaluation
# and not once per pattern text.

ids = []
3.times { ids << /a+b/.object_id }
p ids.uniq.size

# Two literals written in two places are two objects, however identical.
a = /a+b/
b = /a+b/
p a.equal?(b)
p a == b
p a.frozen?

def in_a_method
  /same/
end
p in_a_method.equal?(in_a_method)

# The cached object is the one that matches, and it keeps matching.
r = nil
2.times { r = /(\d+)-(\d+)/ }
m = r.match("10-20")
p [m[1], m[2]]
p r.source
p r.options

# Flags ride with the site.
p /x/i.match?("X")
p /a b/x.match?("ab")
p(/a.b/m.match?("a\nb"))
p(/x/i.options)

# An interpolated literal is rebuilt every time -- a fresh object, and it
# tracks the interpolated value.
outs = []
%w[cat dog].each { |w| outs << /#{w}/.source }
p outs
x = "q"
p(/#{x}/.equal?(/#{x}/))

# A literal inside a block that closes over nothing still caches per site.
seen = []
2.times { seen << /loop/.object_id }
p seen.uniq.size

# Freezing is observable and cannot be undone.
p(/frz/.frozen?)
begin
  /frz/.instance_variable_set(:@x, 1)
rescue FrozenError => e
  p e.class
end
__END__
1
false
true
true
true
["10", "20"]
"(\\d+)-(\\d+)"
0
true
true
true
1
["cat", "dog"]
false
1
true
FrozenError
