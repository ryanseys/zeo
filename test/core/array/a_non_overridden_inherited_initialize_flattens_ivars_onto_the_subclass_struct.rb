class Parent
  def initialize(x)
    @x = x
  end
end
class Child < Parent
  def show
    @x * 10
  end
end
puts Child.new(4).show
__END__
40
