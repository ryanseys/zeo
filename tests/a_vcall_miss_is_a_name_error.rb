# A VCALL is a bare identifier ruby could have read as a local (`foo`, never
# `foo()` or `self.foo`). The lookup is an ordinary send; the MISS is not.
# Ruby cannot tell which the writer meant, so it says "undefined local
# variable or method" and raises NameError. With parens or arguments it is
# unambiguously a call, and a miss is NoMethodError.

def show
  yield
rescue NameError => e
  [e.class, e.message]
end

p(show { nope })
p(show { nope() })
p(show { nope(1) })
p(show { self.nope })

# In a method body, named for the instance.
class K
  def go = show { missing }
  def show = (yield rescue [$!.class, $!.message])
end
p K.new.go

# In a class body, named for the class -- an enclosing local is NOT in scope
# there, which is exactly the case the message is written for.
outer = 1
class Arr
  begin
    outer
  rescue NameError => e
    p [e.class, e.message]
  end
end

# A name that IS a local reads it, no send at all.
defined_local = :read
p(defined_local)

# NameError carries the name it could not find.
begin
  absent_thing
rescue NameError => e
  p [e.name, e.receiver == self]
end
