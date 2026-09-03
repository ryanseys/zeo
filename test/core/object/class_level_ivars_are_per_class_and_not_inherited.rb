# The defining property of class-level `@x`, and the whole reason it
# can't share `@@x`'s storage: a subclass gets its OWN slot, starting
# empty, even though it inherits the method that reads it. Contrast the
# `@@cv` line, which IS shared. Oracle-verified (ruby 4.0.6).

class Base
  @reg = "base-ivar"
  @@cv = "base-cvar"
  def self.reg; @reg; end
  def self.reg=(v); @reg = v; end
  def self.cv; @@cv; end
  def self.unset; @never_written; end
end
class Sub < Base; end
p Base.reg
p Sub.reg
p Sub.cv
p Base.unset
Sub.reg = "sub-only"
p [Base.reg, Sub.reg]
__END__
"base-ivar"
nil
"base-cvar"
nil
["base-ivar", "sub-only"]
