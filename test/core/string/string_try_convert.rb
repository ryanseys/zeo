# `String.try_convert(obj)` -- the `to_str` check-conversion, exposed. Unlike
# `String(obj)` it never falls back to `to_s` and never raises for an object
# that simply isn't string-like; a lying `to_str` is the one error case.

# Already a String: returned as-is (the same object, not a copy).
s = "hello"
p String.try_convert(s)
p String.try_convert(s).equal?(s)

# A subclass is already a String too -- and `try_convert` hands back the very
# object, so it comes out still wearing its own class.
class Name < String; end
sub = Name.new("ada")
p String.try_convert(sub).class
p String.try_convert(sub).equal?(sub)

# Defines `to_str`: converted through it.
class Path
  def to_str = "/tmp/x"
end
p String.try_convert(Path.new)

# ...and a `to_str` that answers a String SUBCLASS satisfies the check without
# being flattened to a plain String.
class Quacker
  def to_str = Name.new("quack")
end
p String.try_convert(Quacker.new).class

# `to_s` alone is NOT the protocol -- only `to_str` is.
class Labelled
  def to_s = "label"
end
p String.try_convert(Labelled.new)

# No conversion at all: nil, never an exception.
p String.try_convert(42)
p String.try_convert(nil)
p String.try_convert([1, 2])
p String.try_convert(:sym)

# A `to_str` that answers a non-String is the one raising case.
class Liar
  def to_str = 42
end
begin
  String.try_convert(Liar.new)
rescue TypeError => e
  puts e.message
end

# `to_str` answering nil reads as "no conversion", not as a lie.
class Absent
  def to_str = nil
end
p String.try_convert(Absent.new)

# The encoding of the converted string survives.
p String.try_convert("caf\xC3\xA9".b).encoding
__END__
"hello"
true
Name
true
"/tmp/x"
Name
nil
nil
nil
nil
nil
can't convert Liar to String (Liar#to_str gives Integer)
nil
#<Encoding:BINARY (ASCII-8BIT)>
