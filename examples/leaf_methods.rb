# A grab-bag of core leaf methods across String, Array, Hash, Symbol, Integer,
# and Proc — each the whole of its behavior, oracle-checked.

# Integer#[] — bit reference (LSB = 0), with (start, len) and range fields.
p 0b1011[0]          # 1
p 5[0, 2]            # 1  -- low two bits
p 255[0..3]          # 15
p 255[4..]           # 15 -- endless range shifts right
p (-2)[0]            # 0  -- two's complement

# Integer bit-mask predicates.
p 0b1010.allbits?(0b0010)   # true
p 0b1010.anybits?(0b0110)   # true
p 0b1010.nobits?(0b0101)    # true

# String leaves.
p "hello".codepoints            # [104, 101, 108, 108, 111]
p "hello".partition("l")        # ["he", "l", "lo"]
p "hello".rpartition("l")       # ["hel", "l", "o"]
p "TestName".delete_prefix("Test")   # "Name"
p "file.rb".delete_suffix(".rb")     # "file"
buf = +"mutable"; buf.clear; p buf   # ""
name = "line\n"; p name.chomp!; p name.chomp!   # "line" then nil (no change)
bs = "hello"; bs.bytesplice(0, 2, "XY"); p bs    # "XYllo"

# Array combinatorics + set predicate + chaining.
p [1, 2].repeated_permutation(2).to_a   # [[1,1],[1,2],[2,1],[2,2]]
p [1, 2].repeated_combination(2).to_a   # [[1,1],[1,2],[2,2]]
p [1, 2, 3].intersect?([3, 4])          # true
p [1, 2].chain([3], [4]).to_a           # [1, 2, 3, 4]

# Hash projections.
p({ a: 1, b: 2, c: 3 }.slice(:a, :c))   # {a: 1, c: 3}
p({ a: 1, b: 2, c: 3 }.except(:b))      # {a: 1, c: 3}
p({ a: 1, b: 2 }.fetch_values(:a, :b))  # [1, 2]
p({ a: 1, b: [2, 3] }.flatten)          # [:a, 1, :b, [2, 3]]

# Symbol name access + case-insensitive compare.
p :hello[1, 3]              # "ell"
p :Hello.casecmp?(:hELLO)   # true

# Proc composition.
inc = ->(x) { x + 1 }
dbl = ->(x) { x * 2 }
p((inc >> dbl).call(3))   # 8  -- dbl(inc(3))
p((inc << dbl).call(3))   # 7  -- inc(dbl(3))

# Regexp source-escaping + flag bitmask.
p Regexp.escape("a.b*c")   # "a\\.b\\*c"
p(/abc/i.options)          # 1  -- IGNORECASE
p(/abc/m.options)          # 4  -- MULTILINE

# Complex cartesian constructor.
p Complex.rect(3, 4)       # (3+4i)

# Range binary search (find-minimum): first element the block accepts.
p((1..100).bsearch { |x| x >= 40 })   # 40
p((1..10).bsearch { |x| x >= 40 })    # nil
