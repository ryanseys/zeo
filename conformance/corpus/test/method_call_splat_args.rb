# Method#[] / #call with a trailing splat expands the array into the
# target's declared params at runtime (#3248).
def add(a, b) = a + b
args = [4, 5]
p(method(:add)[*args])
p(method(:add).call(*args))
p(method(:add).call(1, *[2]))

class C
  def mul(a, b) = a * b
end
m = C.new.method(:mul)
p m.call(*[3, 7])
p m[*[3, 7]]
