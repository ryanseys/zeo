module MA
  def method_added(n) = ((@s ||= []) << n)
  def s = @s
end

class CA
  def a; end
  extend MA
  def b; end
end
p CA.s

module MInc
  def method_added(n) = ((@s ||= []) << n)
  def s = @s
end
module HInc
  def self.included(base) = base.extend(MInc)
end

class CInc
  def a; end
  include HInc
  def b; end
end
p CInc.s

class CReopen
  def a; end
end
class CReopen
  extend MA
  def b; end
end
p CReopen.s

module MS
  def singleton_method_added(n) = ((@s ||= []) << n)
  def s = @s
end
class CS
  def self.a; end
  extend MS
  def self.b; end
end
p CS.s

module MK
  def const_added(n) = ((@s ||= []) << n)
  def s = @s
end
class CK
  K1 = 1
  extend MK
  K2 = 2
end
p CK.s

module MR
  def method_removed(n) = ((@s ||= []) << n)
  def s = @s
end
class CR
  def a; end
  def b; end
  remove_method :a
  extend MR
  remove_method :b
end
p CR.s

module MI
  def inherited(sub) = ((@s ||= []) << sub.to_s)
  def s = @s
end
class CI; end
class DI < CI; end
class CI
  extend MI
end
class EI < CI; end
p CI.s
