class Animal; end
class Dog < Animal; end
class Cat < Animal; end
p(Dog < Animal)
p(Animal < Dog)
p(Dog < Cat)
p(Dog <= Dog)
p(Animal > Dog)
p(Dog <=> Animal)
p(Dog <=> Cat)
p(Dog <=> 5)
p(Integer < Numeric)
begin
  Dog < 5
rescue TypeError => e
  puts e.message
end
class Base; end
class Kid1 < Base; end
class Kid2 < Base; end
class GKid < Kid1; end
p Base.subclasses.length
p Base.subclasses.map { |c| c.to_s }.sort
p Kid1.subclasses.map(&:to_s)
p Base.singleton_class?
__END__
true
false
nil
true
true
-1
nil
nil
true
compared with non class/module
2
["Kid1", "Kid2"]
["GKid"]
false
