# The parameter-capture fix: `n` (a method param) appears ONLY inside
# the escaping block -- previously misclassified as a block-own local
# and silently re-declared Nil. Oracle: 11, 12.

class Pair
  def each(&blk)
    blk.call(1)
    blk.call(2)
    self
  end
  def add_all(n)
    each { |x| puts x + n }
  end
  def mapped(&blk)
    result = []
    each { |x| result << blk.call(x) }
    result
  end
end
Pair.new.add_all(10)
puts Pair.new.mapped { |x| x * 3 }.length
__END__
11
12
2
