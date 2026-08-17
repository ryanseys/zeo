# GAP -- imported from the spinel corpus at c55d9bdb.
# zeo evaluates an endless Enumerable eagerly rather than lazily, so the
# program never terminates and the harness kills it at the 60s deadline.
#
class Inf
  include Enumerable
  def each; i = 1; loop { yield i; i += 1 }; end
end
p(Inf.new.first)
p(Inf.new.first(3))
p(Inf.new.take(3))
p(Inf.new.find { |x| x > 3 })
p(Inf.new.include?(3))
p(Inf.new.take_while { |x| x < 4 })
p(Inf.new.lazy.map { |x| x * 2 }.first(3))
p(Inf.new.each { |x| break x * 10 })

class Fin
  include Enumerable
  def each; yield 1; yield 2; yield 3; end
end
p(Fin.new.first)
p(Fin.new.take(2))
p(Fin.new.include?(9))
p(Fin.new.map { |x| x * 2 })
p(Fin.new.sort)
