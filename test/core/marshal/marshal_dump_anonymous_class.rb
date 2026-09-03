# A dump names the class so `Marshal.load` can find it again, which an
# ANONYMOUS class has no way to be -- so it is refused rather than written as a
# reference nothing can resolve.
def scrub(s) = s.sub(/0x[0-9a-f]+/, "ADDR")
def err
  yield
  :no_raise
rescue TypeError => e
  [e.class, scrub(e.message)]
end

klass = Struct.new(:x)
p err { Marshal.dump(klass.new(42)) }
p err { Marshal.dump(klass) }
p err { Marshal.dump(Class.new) }
p err { Marshal.dump(Module.new) }
p err { Marshal.dump(Class.new.new) }
p err { Marshal.dump(String.singleton_class) }

# A NAMED one still round-trips, however it was minted.
Point = Struct.new(:x, :y)
p Marshal.load(Marshal.dump(Point.new(1, 2))).to_a
Minted = Class.new do
  attr_accessor :v
end
m = Minted.new
m.v = 7
p Marshal.load(Marshal.dump(m)).v
p Marshal.load(Marshal.dump(Point)).name
p Marshal.load(Marshal.dump([1, "two", :three, {a: 1}]))

# An anonymous class nested inside an otherwise fine structure still refuses.
p err { Marshal.dump([1, Class.new.new]) }
p err { Marshal.dump({k: Struct.new(:z).new(1)}) }
__END__
[TypeError, "can't dump anonymous class #<Class:ADDR>"]
[TypeError, "can't dump anonymous class #<Class:ADDR>"]
[TypeError, "can't dump anonymous class #<Class:ADDR>"]
[TypeError, "can't dump anonymous module #<Module:ADDR>"]
[TypeError, "can't dump anonymous class #<Class:ADDR>"]
[TypeError, "singleton class can't be dumped"]
[1, 2]
7
"Point"
[1, "two", :three, {a: 1}]
[TypeError, "can't dump anonymous class #<Class:ADDR>"]
[TypeError, "can't dump anonymous class #<Class:ADDR>"]
