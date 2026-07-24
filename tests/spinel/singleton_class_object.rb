# `class << obj` (block form of def obj.m) on a statically-traceable object.
class Widget
  def initialize(n)
    @n = n
  end
  def label
    "w#{@n}"
  end
end

W = Widget.new(1)
class << W
  def label
    "custom-#{super}"
  end
  def extra
    "x"
  end
end
p W.label
p W.extra
p W.class
p W.is_a?(Widget)
p W.instance_of?(Widget)

# local receiver
class Thing
  def base
    "b"
  end
end
x = Thing.new
class << x
  def special
    "s-#{base}"
  end
end
p x.special

# constant with a body method reached from the singleton method
class Config
  def base
    10
  end
end
CONFIG = Config.new
class << CONFIG
  def reload
    "reload-#{base}"
  end
end
p CONFIG.reload
