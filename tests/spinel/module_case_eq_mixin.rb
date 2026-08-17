class U; end
r001 = (Comparable === U.new rescue $!.class)
p r001

r002 = (Enumerable === U.new rescue $!.class); p r002   # Ruby: false   Spinel: NoMethodError

p(Object === U.new)        # => true
p(Kernel === U.new)        # => true
p(BasicObject === U.new)   # => true
p(Comparable === 1)        # => true
module M; end
p(M === U.new)             # => false
