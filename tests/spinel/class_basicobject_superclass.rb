p BasicObject.superclass
v = BasicObject.superclass; p v
p BasicObject.superclass.nil?
p Object.superclass
p Object.superclass.nil?
# nullable class value works through a variable-held receiver too
w = BasicObject
p w.superclass
p w.superclass.nil?
# and boxed into a container / walked up a user chain to the root
p [Object.superclass, BasicObject.superclass]
class A; end
class B < A; end
p B.superclass.superclass.superclass.superclass
