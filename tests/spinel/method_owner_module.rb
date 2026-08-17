module Greet
  def hello; "hi"; end
end
class Person
  include Greet
end
m = Person.new.method(:hello)
p m.owner
p Person.instance_method(:hello).owner
class Plain2; def own; 1; end; end
p Plain2.new.method(:own).owner
p Person.new.method(:hello).inspect.include?("Greet")
