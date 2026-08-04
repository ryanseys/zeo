# An extension-field element is a fixed-width array of field elements, and its
# add is a generator over the two operands. The generator's own value comes
# from the very methods its result feeds, so answering "array of anything" on
# the round before those settle makes the accumulator poly, which makes these
# parameters poly, which makes `a[i]` poly -- and the cycle has no way back to
# the Integer array it really is.
module F
  def self.add(a, b)
    (a + b) % 97
  end
end

module E
  def self.zero
    [0, 0, 0, 0]
  end

  def self.add(a, b)
    Array.new(4) { |i| F.add(a[i], b[i]) }
  end

  def self.scale(a, s)
    Array.new(4) { |i| F.add(a[i], s) }
  end
end

def run(n, alphas)
  acc = E.zero
  i = 0
  while i < n
    acc = E.add(acc, E.scale(alphas[i % alphas.length], i))
    i += 1
  end
  acc
end

alphas = Array.new(3) { |k| [k, k + 1, k + 2, k + 3] }
p run(50, alphas)
