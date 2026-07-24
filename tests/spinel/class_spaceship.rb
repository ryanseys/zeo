class Animal; end
class Dog < Animal; end
class Cat < Animal; end

p(Dog <=> Animal)
p(Animal <=> Dog)
p(Dog <=> Dog)
p(Dog <=> Cat)
