class Attr
  attr_accessor :x

  def initialize
    @x = 0
  end
end

class Def
  def initialize
    @x1 = 0
  end

  def x
    @x1
  end

  def x=(v)
    @x1 = v
  end
end

class Sub < Attr
  def x=(v)
    @x = v * 2
  end
end

box = [Def.new, Attr.new, Sub.new]

o = box[0]
o.x = 42
puts "def writer  x=#{o.x}"

a = box[1]
a.x = 99
puts "attr writer x=#{a.x}"

s = box[2]
s.x = 21
puts "override    x=#{s.x}"

class Mute
  def initialize
    @y = 0
  end
end

begin
  m = [Mute.new, Attr.new][0]
  m.x = 7
  puts "no raise"
rescue NoMethodError => e
  puts "raised: #{e.class}"
end
