# An exception's message comes from its own `initialize`/`to_s` chain, so
# `raise SomeError` (no message) must actually RUN the class's constructor --
# defaults, `super` and all -- rather than short-cutting to the class name.
# The class-name default lives in Exception#to_s (`@message || self.class.name`).

# Bare `super` forwards the defaulted parameter.
class E < StandardError
  def initialize(m = "def")
    super
  end
end
begin; raise E; rescue => e; p e.message; end
begin; raise E, "x"; rescue => e; p e.message; end
begin; raise E.new; rescue => e; p e.message; end
begin; raise E.new("y"); rescue => e; p e.message; end

# `super("...")` with interpolation, raised argless.
class F < StandardError
  def initialize(m = "dd")
    super("wrapped: #{m}")
  end
end
begin; raise F; rescue => e; p e.message; end
begin; raise F, "given"; rescue => e; p e.message; end

# No custom initialize: the message defaults to the class name.
class G < StandardError; end
begin; raise G; rescue => e; p e.message; end
begin; raise G, "explicit"; rescue => e; p e.message; end

# An inherited user initialize from a user parent.
class Base < StandardError
  def initialize(m = "base-def")
    super
  end
end
class Sub < Base; end
begin; raise Sub; rescue => e; p e.message; end

# A custom initialize that also sets an ivar, raised argless.
class H < StandardError
  def initialize(m = "oops", code = 42)
    @code = code
    super(m)
  end
  attr_reader :code
end
begin; raise H; rescue => e; p [e.message, e.code]; end
begin; raise H.new("boom", 7); rescue => e; p [e.message, e.code]; end

# Built-in raises are unaffected.
begin; raise StandardError; rescue => e; p e.message; end
begin; raise "boom"; rescue => e; p e.message; end
begin; raise ArgumentError; rescue => e; p e.message; end
begin; raise ArgumentError, "bad arg"; rescue => e; p e.message; end
begin; raise RuntimeError.new("built"); rescue => e; p e.message; end

# to_s and message agree, and inspect/class read correctly.
begin
  raise E
rescue => e
  p e.to_s
  p e.message
  p e.class
  p e.class.name
  p e.is_a?(StandardError)
end

# An exception with a custom to_s: message follows it (message calls to_s).
class Custom < StandardError
  def to_s
    "custom-to-s"
  end
end
begin; raise Custom; rescue => e; p [e.message, e.to_s]; end

# A message built from an ivar the constructor set.
class Detailed < StandardError
  def initialize(field)
    @field = field
    super("invalid #{field}")
  end
  attr_reader :field
end
begin
  raise Detailed.new("email")
rescue Detailed => e
  p [e.message, e.field]
end

# Re-raising preserves the same object, message and ivars.
begin
  begin
    raise Detailed.new("name")
  rescue Detailed
    raise
  end
rescue => e
  p [e.message, e.field]
end

# A rescued exception's message survives being stored and re-inspected.
saved = nil
begin; raise E; rescue => e; saved = e; end
p saved.message
