# `class << self; attr_accessor :x; end` is THE idiomatic way to declare
# class-level state, and it works by generating `def self.x; @x; end` --
# so it only works once class-level `@x` has real storage.

module Reg
  class << self
    attr_accessor :handler
    def helper; "helped"; end
  end
end
Reg.handler = "H"
p Reg.handler
p Reg.helper

class Cfg
  class << self
    attr_reader :mode
    attr_writer :mode
  end
  @mode = "default"
end
p Cfg.mode
Cfg.mode = "custom"
p Cfg.mode
__END__
"H"
"helped"
"default"
"custom"
