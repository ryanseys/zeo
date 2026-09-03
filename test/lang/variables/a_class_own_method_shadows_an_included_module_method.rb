module Greetable
  def greet
    "hi"
  end
end
class Person
  include Greetable
  def greet
    "overridden"
  end
end
class Robot
  include Greetable
end
puts Person.new.greet
puts Robot.new.greet
__END__
overridden
hi
