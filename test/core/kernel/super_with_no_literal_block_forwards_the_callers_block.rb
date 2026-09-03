class Parent
  def run
    yield
  end
end
class Child < Parent
  def run
    super
  end
end
puts(Child.new.run { "from-caller" })
__END__
from-caller
