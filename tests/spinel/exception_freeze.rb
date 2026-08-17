# freeze on an exception was a no-op and frozen? a hard-coded false, so an
# exception the program froze itself still read back unfrozen.
p(RuntimeError.new("f").freeze.frozen?)
e = RuntimeError.new("f").freeze
p e.frozen?
class S1 < StandardError; end
s = S1.new("x").freeze
p s.frozen?
p "a".freeze.frozen?
p RuntimeError.new("g").frozen?
begin
  raise "boom"
rescue => x
  p x.frozen?
end
