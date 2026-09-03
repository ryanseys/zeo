# reopening Object (with an include + a def) makes those methods
# dispatch on built-in AND user receivers with the real receiver as self.

module ObjectGreeting
  def hi
    "hi from " + self.class.name
  end
end
class Object
  include ObjectGreeting
  def global_hi
    "global " + self.class.name
  end
end
class LocalThing
end
puts "x".hi
puts "x".global_hi
puts 1.global_hi
puts [1, 2].global_hi
puts LocalThing.new.global_hi
__END__
hi from String
global String
global Integer
global Array
global LocalThing
