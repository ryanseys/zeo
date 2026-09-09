# It defines the accessor there as it does in a literal class body.
klass = Class.new do
  attr_reader :v
  def initialize(v)
    @v = v
  end
end
p klass.new(5).v

Klass2 = Class.new
Klass2.class_eval do
  attr_accessor :y
end
o = Klass2.new
o.y = 99
p o.y
__END__
5
99
