# A local statically holding one class constant dispatches like the constant
# for the reflection surface too (#2717, #2721).
class Dog; def bark; end; end
c = Dog
p c.method_defined?(:bark)
p c.instance_methods(false)
class Base; end
class Kid1 < Base; end
class Kid2 < Base; end
b = Base
p(b.subclasses.length)
p(b.subclasses.map { |k| k.name }.sort)
