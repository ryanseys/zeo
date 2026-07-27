# Marshal.dump on an instance of an anonymous class (e.g. an unassigned
# Struct.new subclass) should raise TypeError ("can't dump anonymous class")
# since there's no name to reference on load. zeo dumps it anyway, embedding
# the anonymous class's placeholder name as if it were loadable.
klass = Struct.new(:x)
obj = klass.new(42)
begin
  Marshal.dump(obj)
rescue => e
  p [e.class, e.message]
end
