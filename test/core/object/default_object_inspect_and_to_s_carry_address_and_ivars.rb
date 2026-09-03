# Default (no override) `to_s` is `#<Class:0xADDR>`; `inspect` adds the
# ivars as `@name=<inspected>` in declaration order. Addresses are
# normalized in-program (as the corpus tests do) so the expectation is
# stable. A frozen-mutation FrozenError message carries the same inspect.

def norm(s) = s.gsub(/0x[0-9a-f]+/, "0xADDR")
class Widget
  def initialize(n); @name = n; @size = 3; end
end
w = Widget.new("gadget")
puts norm(w.to_s)
puts norm(w.inspect)
class Empty; end
puts norm(Empty.new.inspect)
puts norm(Object.new.inspect)
class Frozen
  attr_accessor :v
  def initialize; @v = 1; freeze; end
end
begin
  Frozen.new.v = 2
rescue FrozenError => e
  puts norm(e.message)
end
__END__
#<Widget:0xADDR>
#<Widget:0xADDR @name="gadget", @size=3>
#<Empty:0xADDR>
#<Object:0xADDR>
can't modify frozen Frozen: #<Frozen:0xADDR @v=1>
