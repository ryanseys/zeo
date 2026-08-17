# A user class whose `each` never ends is still usable, because the
# short-circuiting Enumerable methods stop the SOURCE rather than scanning a
# materialized copy of it. `first`/`take`/`find`/`include?` always did;
# `take_while` used to collect first and so ran forever.
#
# The last line is the one that keeps the two kinds of stop apart: a `break` in
# the USER's block belongs to the enclosing call, and must not be read as the
# internal `rb_iter_break` these methods use to halt an endless source.
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
