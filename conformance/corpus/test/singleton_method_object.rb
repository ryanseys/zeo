# Singleton methods on a constant/local that statically holds one object:
# synthesized as an anonymous subclass that masquerades as its parent.
class Config
  def initialize
    @base = 10
  end
  def base
    @base
  end
end

CONFIG = Config.new
def CONFIG.reload
  "reloaded-#{base}"
end
def CONFIG.scale(n)
  base * n
end

p CONFIG.base
p CONFIG.reload
p CONFIG.scale(3)
p CONFIG.class
p CONFIG.class.name
p CONFIG.is_a?(Config)
p CONFIG.instance_of?(Config)

# override a parent method + super
class Widget
  def initialize(n)
    @n = n
  end
  def label
    "widget-#{@n}"
  end
end
W1 = Widget.new(1)
def W1.label
  "custom-#{super}"
end
def W1.tag
  @tag = "T"
  @tag
end
p W1.label
p W1.tag
# poly dispatch alongside a plain instance
[W1, Widget.new(2)].each { |w| puts w.label }

# local receiver
class Thing
  def base
    "b"
  end
end
x = Thing.new
def x.special
  "s-#{base}"
end
p x.special
p x.class

# define_singleton_method
class Box
  def v
    1
  end
end
B = Box.new
B.define_singleton_method(:doubled) { v * 2 }
p B.doubled

# obj.extend(Mod): transplant module methods onto the singleton subclass
module Loud
  def shout
    label.upcase
  end
end
module Soft
  def whisper
    label.downcase
  end
end
class Speaker
  def initialize(s)
    @s = s
  end
  def label
    @s
  end
end
SPK = Speaker.new("Hi")
SPK.extend(Loud)
SPK.extend(Soft)
def SPK.both
  "#{shout}/#{whisper}"
end
p SPK.shout
p SPK.whisper
p SPK.both
p SPK.class
