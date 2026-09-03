# A STARK prover's inner loop, in the shape real ones are written in: a prime
# field as module functions over plain Integers, a degree-4 extension field as
# length-4 Integer arrays, an NTT over an int array, and a transition table as
# an array of int arrays.
#
# The point of the benchmark is TYPES, not throughput. Every value here is an
# Integer or an array of them, so it measures how much of that a backend can
# keep unboxed. Imported from the predecessor project, where it was a
# regression gate for a type inferencer; the shapes it collects are the ones
# that defeated it, and each is still a fair question to ask zeo:
#
#   - `acc = ExtField.add(acc, x)` -- a value flowing back into the parameter
#     it came from
#   - `xs.map { Field.sub(...) }` -- an element type derived from a block
#   - `return h[k] if h.key?(k) ... raise` -- a guarded return with a raising
#     tail
#   - `labels.key?(name)` before `labels[name] = int` -- a hash first seen
#     through a read
#   - `Array.new(n) { Array.new(n, 0) }` -- a nested int table
#
# zeo has no type inferencer of that kind: its `TyKind` map is per-local and
# the CLIF emitter does not consult it yet (the recorded perf lever). So this
# runs as a shape benchmark today and becomes a gate if that lever lands.
#
# Sized so CRuby finishes in about a second.

module Field
  P = 2013265921   # BabyBear: 15 * 2^27 + 1
  G = 31           # a generator of F_p*

  def self.add(a, b) = (a + b) % P
  def self.sub(a, b) = (a + P - b) % P
  def self.mul(a, b) = (a * b) % P

  def self.pow(a, e)
    r = 1
    base = a
    x = e
    while x > 0
      r = (r * base) % P if x & 1 == 1
      base = (base * base) % P
      x >>= 1
    end
    r
  end

  def self.inv(a) = pow(a, P - 2)

  # omega of order 2^k
  def self.root(k) = pow(G, (P - 1) / (1 << k))
end

# F_{p^4} = F_p[X]/(X^4 - 11), elements as [c0, c1, c2, c3]
module ExtField
  W = 11

  def self.zero = [0, 0, 0, 0]

  def self.add(a, b)
    [Field.add(a[0], b[0]), Field.add(a[1], b[1]),
     Field.add(a[2], b[2]), Field.add(a[3], b[3])]
  end

  def self.scale(a, s)
    [Field.mul(a[0], s), Field.mul(a[1], s),
     Field.mul(a[2], s), Field.mul(a[3], s)]
  end

  def self.mul(a, b)
    d0 = Field.add(Field.mul(a[0], b[0]),
                   Field.mul(W, Field.add(Field.mul(a[1], b[3]),
                                          Field.add(Field.mul(a[2], b[2]), Field.mul(a[3], b[1])))))
    d1 = Field.add(Field.add(Field.mul(a[0], b[1]), Field.mul(a[1], b[0])),
                   Field.mul(W, Field.add(Field.mul(a[2], b[3]), Field.mul(a[3], b[2]))))
    d2 = Field.add(Field.add(Field.mul(a[0], b[2]),
                             Field.add(Field.mul(a[1], b[1]), Field.mul(a[2], b[0]))),
                   Field.mul(W, Field.mul(a[3], b[3])))
    d3 = Field.add(Field.add(Field.mul(a[0], b[3]), Field.mul(a[1], b[2])),
                   Field.add(Field.mul(a[2], b[1]), Field.mul(a[3], b[0])))
    [d0, d1, d2, d3]
  end
end

module Poly
  # in-place radix-2 NTT
  def self.ntt!(values, omega)
    n = values.length
    i = 0
    j = 0
    while i < n - 1
      bit = n >> 1
      while j >= bit
        j -= bit
        bit >>= 1
      end
      j += bit
      if i < j
        t = values[i]
        values[i] = values[j]
        values[j] = t
      end
      i += 1
    end

    len = 2
    while len <= n
      wlen = Field.pow(omega, n / len)
      half = len >> 1
      start = 0
      while start < n
        w = 1
        k = 0
        while k < half
          u = values[start + k]
          v = Field.mul(values[start + k + half], w)
          values[start + k] = Field.add(u, v)
          values[start + k + half] = Field.sub(u, v)
          w = Field.mul(w, wlen)
          k += 1
        end
        start += len
      end
      len <<= 1
    end
    values
  end

  # one inversion for the whole batch (Montgomery's trick)
  def self.batch_inv(xs)
    n = xs.length
    prefix = Array.new(n, 1)
    acc = 1
    i = 0
    while i < n
      prefix[i] = acc
      acc = Field.mul(acc, xs[i])
      i += 1
    end
    inv_acc = Field.inv(acc)
    out = Array.new(n, 0)
    i = n - 1
    while i >= 0
      out[i] = Field.mul(inv_acc, prefix[i])
      inv_acc = Field.mul(inv_acc, xs[i])
      i -= 1
    end
    out
  end
end

# A tiny program whose jump targets come from a label table: the shape that
# makes a hash pick its variant from a read rather than from its writes.
class Program
  def initialize(lines)
    @lines = lines
  end

  def labels
    t = {}
    i = 0
    while i < @lines.length
      name = @lines[i]
      raise "duplicate label #{name}" if t.key?(name)

      t[name] = i * 3
      i += 1
    end
    t
  end

  def target(token, table)
    return table[token] if table.key?(token)
    return token.to_i if token != ""

    raise ArgumentError, "unknown target #{token}"
  end
end

# Transition table as an array of int arrays, filled by index.
class Air
  def initialize(rows)
    @rows = rows
    @t = Array.new(rows) { Array.new(rows, 0) }
    k = 0
    while k < rows
      @t[k][(k + 1) % rows] = Field.add(@t[k][(k + 1) % rows], 1)
      @t[k][k] = Field.sub(@t[k][k], k + 1)
      k += 1
    end
  end

  def fingerprint
    s = 0
    r = 0
    while r < @rows
      row = @t[r]
      cidx = 0
      while cidx < @rows
        s = Field.add(Field.mul(s, 31), row[cidx])
        cidx += 1
      end
      r += 1
    end
    s
  end
end

LOG_N = 18
N = 1 << LOG_N

# --- trace + NTT
omega = Field.root(LOG_N)
trace = Array.new(N, 0)
x = 1
i = 0
while i < N
  trace[i] = x
  x = Field.add(Field.mul(x, 3), 7)
  i += 1
end
evals = Poly.ntt!(trace, omega)

# --- batch inversion over a mapped array (element type not known up front)
shifted = evals.map { |e| Field.sub(e, 1) }
i = 0
while i < N
  shifted[i] = 1 if shifted[i] == 0
  i += 1
end
inv = Poly.batch_inv(shifted)

# --- extension-field accumulator: the value flows back into its own parameter
acc = ExtField.zero
i = 0
while i < N
  acc = ExtField.add(acc, ExtField.scale([evals[i], inv[i], i % 97, 1], 7))
  acc = ExtField.mul(acc, [1, 1, 0, 0]) if i % 128 == 0
  i += 1
end

# --- label table + guarded lookup
names = []
i = 0
while i < 64
  names.push("L#{i}")
  i += 1
end
prog = Program.new(names)
table = prog.labels
sum = 0
i = 0
while i < 64
  sum = Field.add(sum, prog.target("L#{i}", table))
  sum = Field.add(sum, prog.target(i.to_s, table))
  i += 1
end

air = Air.new(1800)

puts acc.inspect
puts Field.add(inv[0], inv[N - 1])
puts sum
puts air.fingerprint
__END__
[673065045, 1857861762, 1544397081, 339589583]
864836275
8064
1939952147
