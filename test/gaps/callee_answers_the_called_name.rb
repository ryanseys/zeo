# `__callee__` answers the name a method was CALLED through and `__method__`
# the name it was defined as, so an alias makes them differ. Four aliases:
# on a singleton class, on a block-defined method, on a per-object
# singleton, and one installed at run time. zeo's frame carries one label,
# so `__callee__` answers the defined name everywhere.
class Klass
  def self.original = __callee__
  singleton_class.alias_method :nickname, :original
end
p [Klass.original, Klass.nickname]

[1].each do
  def blk = [__method__, __callee__]
  alias blk_alias blk
end
p [blk, blk_alias]

o = Object.new
def o.sing = [__method__, __callee__]
o.singleton_class.alias_method :sing_alias, :sing
p [o.sing, o.sing_alias]

class Runtime
  def base = __callee__
end
Runtime.send(:alias_method, :late, :base)
p [Runtime.new.base, Runtime.new.late]
__END__
[:original, :nickname]
[[:blk, :blk], [:blk, :blk_alias]]
[[:sing, :sing], [:sing, :sing_alias]]
[:base, :late]
