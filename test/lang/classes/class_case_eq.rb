class Animal; end
class Dog < Animal; end
class Cat; end
d = Dog.new
p(Animal === d)
p(Cat === d)
p(Dog === d)
p(Comparable === 5)
__END__
true
false
true
true
