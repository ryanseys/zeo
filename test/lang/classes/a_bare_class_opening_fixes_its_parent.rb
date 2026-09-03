# The shapes around `superclass mismatch`. A `class Foo` with no clause gives
# Foo the class Object as its parent, and that is a decision, not an absence:
# every later reopen must agree with it or raise.
class Parent; end
class Other; end

class Bare; end
begin
  class Bare < Parent; end
rescue TypeError => e
  p [e.class, e.message]
end
p Bare.superclass.to_s

# Restating the SAME parent is fine, however many times, and a bare reopen of
# a declared class agrees with it by saying nothing.
class Kid < Parent
  def a = 1
end
class Kid < Parent
  def b = 2
end
class Kid
  def c = 3
end
k = Kid.new
p [k.a, k.b, k.c, Kid.superclass.to_s]

# Two DIFFERENT declared parents conflict.
begin
  class Kid < Other; end
rescue TypeError => e
  p [e.class, e.message]
end

# A module has no superclass to disagree about.
module Mixin
  def m = :m
end
module Mixin
  def n = :n
end
class UsesMixin
  include Mixin
end
p [UsesMixin.new.m, UsesMixin.new.n]

# A builtin reopen may restate its real parent, and may not invent one.
class String < Object
  def shout = upcase + "!"
end
p "hi".shout
begin
  class String < Parent; end
rescue TypeError => e
  p [e.class, e.message]
end

# The KIND has to agree too, and ruby names the leaf.
begin
  module Parent; end
rescue TypeError => e
  p [e.class, e.message.lines.first.chomp]
end
__END__
[TypeError, "superclass mismatch for class Bare"]
"Object"
[1, 2, 3, "Parent"]
[TypeError, "superclass mismatch for class Kid"]
[:m, :n]
"HI!"
[TypeError, "superclass mismatch for class String"]
[TypeError, "Parent is not a module"]
