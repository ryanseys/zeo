# A module's `def initialize` runs when an includer is constructed, and the
# module itself has no `new`. Spinel emitted a constructor for the module all
# the same, whose body called an sp_<Module>_initialize that nothing defines --
# a module's methods are emitted into each includer, not under the module's own
# name -- so a compiler that rejects implicit declarations stopped the build.
# `M.new` used to build one quietly; it is a NoMethodError in Ruby (#4167).
module Outer
  module Buffering
    def initialize
      @eof = false
      @rbuffer = "".dup
      @sync = false
    end

    def eof?
      @eof
    end

    def buf
      @rbuffer
    end
  end
end

class Sock
  include Outer::Buffering

  def initialize
    super
    @extra = 1
  end

  def extra
    @extra
  end
end

class Plain
  include Outer::Buffering
end

s = Sock.new
p s.eof?
p s.buf
p s.extra

q = Plain.new
p q.eof?
p q.buf

begin
  Outer::Buffering.new
rescue NoMethodError
  puts "NoMethodError"
end
