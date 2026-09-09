# Marshal's entry surface beyond the byte format (which matches
# exactly): `dump(obj, io)` WRITES to the IO (zeo writes nothing),
# `load` accepts an IO source, `freeze: true` freezes the graph, the
# proc argument visits every object, the depth-limit argument raises
# ArgumentError past the limit, and the incompatible-format TypeError
# carries its detail line ("format version 4.8 required; 9.8 given").
require "stringio"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { io = StringIO.new(+"".b); Marshal.dump([1], io); io.string.bytes.first(2) }
show { Marshal.load(StringIO.new(Marshal.dump(:sym))) }
show { r = Marshal.load(Marshal.dump(["s"]), freeze: true); [r.frozen?, r.first.frozen?] }
show do
  seen = []
  Marshal.load(Marshal.dump([1, "a"]), ->(o) { seen << o.class; o })
  seen.map(&:to_s).sort
end
show { Marshal.dump([[1]], 1) }
show { Marshal.load("\x09\x08T") }
__END__
[4, 8]
:sym
[true, true]
["Array", "Integer", "String", "TrueClass"]
ArgumentError: exceed depth limit
TypeError: incompatible marshal file format (can't be read)
	format version 4.8 required; 9.8 given
