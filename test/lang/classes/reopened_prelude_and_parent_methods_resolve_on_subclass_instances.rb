# The dispatch guard tests for the MRO-walk fallback (`lookup_mro`):
# a reopened exception-prelude class's method visible on rescued
# subclass instances, late parent reopens and mid-chain module
# includes visible on deep-leaf instances. Expected output is
# verbatim ruby 4.0.6.

class StandardError
  def tagged; "SE-tag: #{message}"; end
end
begin
  raise ArgumentError, "boom"
rescue => e
  puts e.tagged
end
class Base
  def hello; "base hello"; end
end
class Mid < Base; end
class Leaf < Mid; end
puts Leaf.new.hello
class Base
  def late; "late method"; end
end
puts Leaf.new.late
module Mixin
  def mixed; "mixed in"; end
end
class Mid
  include Mixin
end
puts Leaf.new.mixed
class Exception
  def exc_tag; "exc: #{self.class}"; end
end
begin
  raise "r"
rescue => e
  puts e.exc_tag
end
__END__
SE-tag: boom
base hello
late method
mixed in
exc: RuntimeError
