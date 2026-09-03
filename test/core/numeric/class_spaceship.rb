class Animal; end
class Dog < Animal; end
class Cat < Animal; end

p(Dog <=> Animal)
p(Animal <=> Dog)
p(Dog <=> Dog)
p(Dog <=> Cat)
__END__
-1
1
0
nil
