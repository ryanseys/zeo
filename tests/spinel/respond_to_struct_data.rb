# `respond_to?` on a class receiver answered false for class methods the same
# program calls: `Struct.new`, `Data.define`, and a generated struct class's
# own `members` (#3482). Neighbouring core classes already answered true, so
# these were the odd ones out.
S = Struct.new(:x, :y)
D = Data.define(:z)

p S.new(1, 2).x
p S.members
p D.new(z: 3).z

p Struct.respond_to?(:new)
p Struct.respond_to?(:members)
p Data.respond_to?(:define)
p S.respond_to?(:members)
p D.respond_to?(:members)
p S.respond_to?(:new)
p D.respond_to?(:new)

# the neighbours that already answered
p Array.respond_to?(:new)
p Hash.respond_to?(:[])
p String.respond_to?(:new)

# and names a class object genuinely does not answer
p S.respond_to?(:no_such_method)
p Struct.respond_to?(:no_such_method)
p Data.respond_to?(:no_such_method)

# an instance still answers its own surface
s = S.new(1, 2)
p s.respond_to?(:members)
p s.respond_to?(:x)
p s.respond_to?(:to_h)
p s.respond_to?(:no_such_method)
