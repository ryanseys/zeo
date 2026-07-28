# `Klass.try_convert(obj)` for the other three types that expose the
# check-conversion protocol. All four doors share one implementation, and its
# defining property is that the answer keeps the class it arrived with: the
# type check is on the underlying representation, which a subclass instance
# satisfies without needing a conversion at all.

class Rows < Array; end
class Index < Hash; end

rows = Rows.new([1, 2])
p Array.try_convert(rows).class
p Array.try_convert(rows).equal?(rows)

index = Index.new
index[:a] = 1
p Hash.try_convert(index).class
p Hash.try_convert(index).equal?(index)

# The duck side of each protocol.
class Pair
  def to_ary = [:l, :r]
end
p Array.try_convert(Pair.new)

class Config
  def to_hash = { mode: :fast }
end
p Hash.try_convert(Config.new)

class Pattern
  def to_regexp = /ab+/i
end
p Regexp.try_convert(Pattern.new)
p Regexp.try_convert(/lit/)

# ...and a duck answering a SUBCLASS still satisfies the check as itself.
class Wrapped
  def to_ary = Rows.new([:only])
end
p Array.try_convert(Wrapped.new).class

# No conversion available: nil, never an exception.
p Array.try_convert(nil)
p Array.try_convert("abc")
p Hash.try_convert([[1, 2]])
p Regexp.try_convert("ab+")
p Regexp.try_convert(nil)

# A lying converter is the one raising case, per protocol.
class Fibber
  def to_ary = :nope
end
begin
  Array.try_convert(Fibber.new)
rescue TypeError => e
  puts e.message
end

# ...and one answering nil reads as "no conversion", not as a lie.
class Silent
  def to_hash = nil
end
p Hash.try_convert(Silent.new)
