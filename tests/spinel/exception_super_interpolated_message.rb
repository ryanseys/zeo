# A user exception subclass whose #initialize forwards an interpolated
# string to super must report that interpolated message, with the
# construction args bound to the initialize params -- not just the
# first construction arg. Covers both raise E.new(...) and an assigned
# E.new(...). Regression for #1048.
class NotFoundError < StandardError
  def initialize(kind, id)
    super("not found - kind `#{kind}`, id `#{id}`")
  end
end

begin
  raise NotFoundError.new("company", "123")
rescue => e
  puts e.message
end

err = NotFoundError.new("user", "42")
puts err.message

# Plain `super(msg)` (no interpolation) must still report the message.
class Plain < StandardError
  def initialize(msg)
    super(msg)
  end
end
begin
  raise Plain.new("plain message")
rescue => e
  puts e.message
end

# A default-valued param used in the super message resolves to its
# default when no construction arg is given.
class Defaulted < StandardError
  def initialize(detail = "fallback")
    super("oops: #{detail}")
  end
end
puts Defaulted.new.message
puts Defaulted.new("custom").message
