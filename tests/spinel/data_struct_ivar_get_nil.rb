# Data/Struct members are not @-instance variables: a literal
# instance_variable_get answers nil and instance_variables is [] (#2849).
Point = Data.define(:x, :y)
p Point.new(1, 2).instance_variable_get(:@x)
S = Struct.new(:x, :y)
p S.new(10, 20).instance_variable_get(:@x)
p S.new(1, 2).instance_variables
class PL; def initialize; @z = 5; end; end
p PL.new.instance_variable_get(:@z)
p PL.new.instance_variables
p Point.new(1, 2).x
