class Foo
  def self.base
    100
  end
  def self.greet(name)
    "hello #{name}"
  end
  def self.tag(pfx = "t")
    pfx + "-foo"
  end
end
class Bar < Foo
  def self.base
    super * 10
  end
  def self.greet(name)
    super(name.upcase) + "!"
  end
  def self.tag(pfx = "t")
    super
  end
end
class Baz < Bar
  def self.base
    super + 1
  end
end
p Foo.base
p Bar.base
p Baz.base
p Bar.greet("matz")
p Bar.tag
p Bar.tag("x")
