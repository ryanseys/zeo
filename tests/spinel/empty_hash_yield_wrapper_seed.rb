# An empty {} passed as a yield-wrapper's accumulator seed: the callee builds
# it through its block (acc = yield(acc, x)), so the param/return can't settle
# from the wrapper body (the precise index-write widening can't reach it).
# It must not degrade the wrapper -- and its caller -- to a void method.
class Rel
  def initialize(a)
    @a = a
  end
  def inject(acc)
    @a.each { |x| acc = yield(acc, x) }
    acc
  end
end

class V
  def self.tally(flag)
    if flag
      {}
    else
      Rel.new([1, 2]).inject({}) do |memo, v|
        memo[v] = { a: v }
        memo
      end
    end
  end
end

puts V.tally(true).length
puts V.tally(false).length
h = V.tally(false)
p h[1]
p h.class
