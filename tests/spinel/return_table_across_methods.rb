# Factoring a table of int arrays out into its own method is the ordinary way
# to write this, and the caller's `rows = build(n)` has to reach the same
# storage the callee built. Without a slot for the method's value that write
# was an unmodeled source, the caller's table stayed a boxed poly array, and
# every helper reading an element out of it bound a boxed parameter.
class F
  def self.mul(a, b)
    (a * b) % 97
  end
end

class T
  def self.build(count)
    Array.new(count) { |c| [c, c + 1, c + 2] }
  end

  def self.consume(rows, s)
    acc = 0
    i = 0
    while i < rows.length
      row = rows[i]
      acc += F.mul(row[0], s)
      i += 1
    end
    acc
  end
end

rows = T.build(4)
p T.consume(rows, 3)
