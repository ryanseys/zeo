# Class#allocate on builtin classes yields the class's empty value, and a
# variable-held class (builtin or user) dispatches like the constant (#2655).
# A user allocate skips initialize.
p String.allocate
p Array.allocate
p Hash.allocate
class Widget
  def initialize; @x = 1; end
end
w = Widget
p w.allocate.class
s = String
p s.allocate
p Widget.allocate.class
