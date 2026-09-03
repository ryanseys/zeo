# A class/module is ordered against another by the ancestry relation: `<` is a
# proper subclass, `<=` allows equality, `>`/`>=` are the reverse, and an
# unrelated pair is `nil` (not false). `<=>` folds the four into -1/0/1/nil.
class Animal; end
class Dog < Animal; end
class Cat < Animal; end

p(Dog < Animal)      # true  -- proper subclass
p(Animal < Dog)      # false -- Animal is the ancestor
p(Dog < Dog)         # false -- not proper
p(Dog <= Dog)        # true
p(Animal > Dog)      # true
p(Dog >= Animal)     # false
p(Dog < Cat)         # nil   -- unrelated
p(Dog <=> Animal)    # -1
p(Animal <=> Dog)    # 1
p(Dog <=> Dog)       # 0
p(Dog <=> Cat)       # nil
p(Dog <=> 5)         # nil   -- non-module argument

# Builtins participate through the same ancestry chain.
p(Integer < Numeric)     # true
p(Comparable > Integer)  # true
p(String <= Comparable)  # true

# A non-class/module argument to `<`/`<=`/`>`/`>=` is a TypeError.
begin
  Dog < 5
rescue TypeError => e
  puts e.message
end

# `subclasses` lists the direct, currently-defined subclasses.
class Base; end
class Kid1 < Base; end
class Kid2 < Base; end
class GKid < Kid1; end
p Base.subclasses.length
p Base.subclasses.map { |c| c.to_s }.sort
p Kid1.subclasses.map { |c| c.to_s }
p Kid2.subclasses

# Named classes and modules are never singleton classes.
p Dog.singleton_class?
module M; end
p M.singleton_class?
__END__
true
false
false
true
true
false
nil
-1
1
0
nil
nil
true
true
true
compared with non class/module
2
["Kid1", "Kid2"]
["GKid"]
[]
false
false
