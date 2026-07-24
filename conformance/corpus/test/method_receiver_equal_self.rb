# method(:m).receiver of a bare top-level method IS the main object, so the
# identity test against self folds to true (#3245).
def d(n) = n
p(method(:d).receiver.equal?(self))
p(method(:d).receiver == self)

class C
  def m = 1
end
o = C.new
p(o.method(:m).receiver.equal?(o))
