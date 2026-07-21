# Method#owner is the defining class (the receiver's class for a builtin
# method object); #receiver is the bound receiver (#2701).
m = 5.method(:+)
p m.owner
p m.receiver
p "hi".method(:upcase).owner
p "hi".method(:upcase).receiver
class U; def f; 1; end; end
u = U.new
mu = u.method(:f)
p mu.owner
p mu.receiver.class
class Sub < U; end
p Sub.new.method(:f).owner
