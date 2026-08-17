# A keyword key binds by name only to a parameter that IS a keyword. A
# positional parameter merely sharing the name takes the whole hash
# positionally, the way any other unconsumed keyword hash does. Matching on the
# name alone made the call look like it supplied no positional argument: the
# value came back 0, and where the callee had a second parameter the compiler
# reported a missing keyword instead of the arity error CRuby raises.
def top(x)
  x
end
p top(x: 9)

class C
  def self.f(attrs)
    attrs
  end

  def g(opts)
    opts
  end

  def h(a, b)
    [a, b]
  end
end

p C.f(attrs: 1)
p C.new.g(opts: 2)
p C.new.h(1, b: 2)

# a real keyword parameter of the same name still binds by name
class D
  def self.f(x: 0)
    x
  end

  def self.g(a, x: 0)
    [a, x]
  end
end

p D.f(x: 9)
p D.g(1, x: 9)

# and the arity error is the one CRuby raises, not a missing-keyword report
class E
  def self.f(attrs, extra)
    [attrs, extra]
  end
end

begin
  E.f(attrs: 1)
  puts "no raise"
rescue ArgumentError => e
  puts e.message
end
