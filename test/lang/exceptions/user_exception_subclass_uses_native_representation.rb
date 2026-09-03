# A USER exception subclass is backed by the native `RubyException` (D3): a
# custom ivar, `super` into the native `initialize` (which stores the message
# in its hidden slot), and reflection all match CRuby -- `instance_variables`
# is `[:@code]` only (NOT `@message`), and `@message` reads nil while
# `#message` returns the super-provided text.

class MyErr < StandardError
  def initialize(code)
    @code = code
    super("boom #{code}")
  end
  def code
    @code
  end
end

e = MyErr.new(42)
puts e.message
puts e.code
puts e.is_a?(StandardError)
p e.instance_variables
p e.instance_variable_get(:@message)
p e.instance_variable_get(:@code)
begin
  raise MyErr, "direct"
rescue StandardError => ex
  puts "#{ex.class}: #{ex.message}"
end
__END__
boom 42
42
true
[:@code]
nil
42
MyErr: boom direct
