# An over- or under-supplied instance call went through with the extra
# arguments simply dropped, where the free-function path already raised.
class K
  def g(a); a; end
  def h(a, b); [a, b]; end
  def opt(a, b = 2); [a, b]; end
  def lead(x = 1, y); [x, y]; end
  def rest(*a); a; end
end

k = K.new
p((k.g(1, 2) rescue $!.class))
p((k.g rescue $!.class))
p((k.h(1) rescue $!.class))
p((k.h(1, 2, 3) rescue $!.message))
p k.g(1)
p k.h(1, 2)
p k.opt(1)
p k.opt(1, 3)
p((k.opt(1, 2, 3) rescue $!.class))
p k.lead(8)
p k.lead(7, 8)
p k.rest(1, 2, 3)
p k.rest

require 'set'
p((Set[1].union([2], [3]).to_a rescue $!.class))
p(Set[1].union([2]).to_a)
