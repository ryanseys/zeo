# Array#slice!(from, n) on every typed array. Returns a fresh
# array of n elements starting at `from`, and mutates the receiver
# to remove them. IntArray uses its `start` field for an O(1) head
# peel; the others shift the tail down to fill the hole.

# --- IntArray: head peel (from == 0) ---
arr = [10, 20, 30, 40, 50]
head = arr.slice!(0, 2)
puts head.length        # 2
puts head[0]            # 10
puts head[1]            # 20
puts arr.length         # 3
puts arr[0]             # 30

# --- IntArray: middle slice (from > 0, tail shift) ---
arr2 = [1, 2, 3, 4, 5, 6, 7]
mid = arr2.slice!(2, 3)
puts mid[0]             # 3
puts mid[2]             # 5
puts arr2.length        # 4
puts arr2[2]            # 6 (tail shifted down)
puts arr2[3]            # 7

# --- IntArray: n > available, clamped ---
arr3 = [100, 200, 300]
big = arr3.slice!(1, 999)
puts big.length         # 2
puts arr3.length        # 1

# --- FloatArray ---
fa = [1.5, 2.5, 3.5, 4.5, 5.5]
fh = fa.slice!(0, 2)
puts fh.length          # 2
puts fh[0]              # 1.5
puts fa.length          # 3
puts fa[0]              # 3.5

# --- StrArray ---
sa = ["a", "b", "c", "d", "e"]
sh = sa.slice!(1, 2)
puts sh.length          # 2
puts sh[0]              # b
puts sh[1]              # c
puts sa.length          # 3
puts sa[0]              # a
puts sa[1]              # d (tail shifted down)
puts sa[2]              # e

# --- SymArray (shares the IntArray helper internally) ---
syms = [:foo, :bar, :baz, :qux]
sym_h = syms.slice!(0, 2)
puts sym_h.length       # 2
puts sym_h[0]           # foo
puts syms.length        # 2
puts syms[0]            # baz
puts syms[1]            # qux

# --- PtrArray (array of obj_X instances) ---
class Box
  attr_reader :v
  def initialize(v); @v = v; end
end

pa = [Box.new(10), Box.new(20), Box.new(30), Box.new(40)]
ph = pa.slice!(1, 2)
puts ph.length          # 2
puts ph[0].v            # 20
puts ph[1].v            # 30
puts pa.length          # 2
puts pa[0].v            # 10
puts pa[1].v            # 40 (tail shifted down)

# --- PolyArray (heterogeneous literal) ---
poly = [1, "two", :three, 4.5, "five"]
poly_h = poly.slice!(1, 3)
puts poly_h.length      # 3
puts poly_h[0]          # two
puts poly_h[2]          # 4.5
puts poly.length        # 2
puts poly[0]            # 1
puts poly[1]            # five
