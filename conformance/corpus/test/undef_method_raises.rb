# Issue #717. `undef <m>` removes the method from the class's
# dispatch table AND a subsequent call to it raises NoMethodError
# (matching CRuby). spinel previously recorded the undef but the
# slot stayed callable.

class C
  def meth; "method"; end
  undef meth
end

begin
  puts C.new.meth
rescue NoMethodError
  puts "NoMethodError"
end

# Inheritance: child class's `undef <parent_method>` shadows the
# parent. Calling on a Child instance must still raise; calling on
# a parent instance still works.
class P
  def hello; "hi"; end
end

class Child < P
  undef hello
end

begin
  puts Child.new.hello
rescue NoMethodError
  puts "Child NoMethodError"
end

puts P.new.hello
