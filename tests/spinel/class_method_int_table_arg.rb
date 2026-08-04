# A namespace module whose methods are all `self.` ones passes a table of int
# arrays between two of them. Emission resolves `M.consume(...)` through the
# class-method chain, so the narrowing pass has to resolve it the same way --
# otherwise the argument is an unattributable escape, the table stays a boxed
# poly array, and every helper it feeds binds a boxed parameter.
class F
  def self.mul(a, b)
    (a * b) % 97
  end
end

class M
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

rows = Array.new(4) { |c| [c, c + 1, c + 2] }
p M.consume(rows, 3)
