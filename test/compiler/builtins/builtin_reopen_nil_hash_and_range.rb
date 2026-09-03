# The receiver-kind breadth: NilClass (no static TyKind -- fully dynamic),
# Hash (implicit native `size`), and Range (static `self.last`/`self.first`
# fast paths inside the reopen body).

class NilClass
  def describe
    "nothing"
  end
end

class Hash
  def pair_count
    size
  end
end

class Range
  def span
    self.last - self.first
  end
end

puts nil.describe
puts({ a: 1, b: 2 }.pair_count)
puts (3..9).span
__END__
nothing
2
6
