# Ruby's exception hierarchy is not a chain: a class can include modules, and
# `rescue M` / `#is_a?(M)` must agree about them.
module Retryable; end
class NetGlitch < StandardError
  include Retryable
end
begin
  raise NetGlitch, "x"
rescue Retryable
  puts "rescued via module"
rescue => e
  p ["fell through", e.class]
end
begin
  raise NetGlitch, "x"
rescue => e
  p e.is_a?(Retryable)
  p e.is_a?(NetGlitch)
  p e.is_a?(StandardError)
end
p NetGlitch.ancestors.first(4)

# the readiness family: an Errno subclass that also includes a module, with the
# module reached from two different Errno parents
require "socket"
begin
  raise IO::EAGAINWaitReadable, "x"
rescue IO::WaitReadable => e
  p ["module", e.class]
end
begin
  raise IO::EAGAINWaitReadable, "x"
rescue Errno::EAGAIN
  p "errno"
end
begin
  raise IO::EAGAINWaitReadable, "x"
rescue SystemCallError
  p "syscall"
end
begin
  raise IO::EAGAINWaitReadable, "x"
rescue => e
  p e.is_a?(IO::WaitReadable)
  p e.is_a?(Errno::EAGAIN)
  p e.is_a?(SystemCallError)
  p e.is_a?(IO::WaitWritable)
end
begin
  raise IO::EINPROGRESSWaitWritable, "y"
rescue IO::WaitWritable => e
  p ["w", e.class]
end
# Errno::EWOULDBLOCK IS Errno::EAGAIN in CRuby
begin
  raise Errno::EAGAIN, "z"
rescue Errno::EWOULDBLOCK
  p "EWOULDBLOCK catches EAGAIN"
end
