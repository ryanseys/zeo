class Parent
  def run
    yield
  end
  def each_twice
    yield 1
    yield 2
  end
end
class Child < Parent
  def run
    super { "from-child" }
  end
  def each_twice
    super { |v| puts "got #{v}" }
  end
end
puts Child.new.run
Child.new.each_twice
__END__
from-child
got 1
got 2
