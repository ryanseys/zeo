# A class method a subclass inherits runs with self = that subclass: CRuby's
# `def self.bench_name; self.name; end` answers the subclass's name. The
# defining class's name was folded in at compile time, so a subclass that did
# not override the method reported the base class.

class Base
  def self.bench_name
    name.to_s
  end

  def self.describe
    "<#{bench_name}>"
  end

  def label
    self.class.bench_name
  end
end

class Sub < Base
  def self.bench_name
    "sub!"
  end
end

class Sub2 < Base
end

def pick(f)
  f ? Sub.new : Sub2.new
end

p pick(true).label
p pick(false).label
p Sub2.bench_name
p Sub.describe
p Sub2.describe
