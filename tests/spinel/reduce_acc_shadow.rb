# Array#reduce: the accumulator block param's type must come from the
# seed (init_val), not from the element type. infer_type(acc) inside
# the block must follow the seed in both directions — pointer seed
# (string) over a value-typed outer, and value seed (int) over a
# pointer-typed outer.

def f
  total = [1, 2, 3].reduce("") { |acc, x| acc + x.to_s }
  puts total   # 123
end
f

# Outer same-named local of a different type must not leak into the block.
def g
  acc = 99
  out = [10, 20, 30].reduce("=") { |acc, x| acc + x.to_s }
  puts out   # =102030
  puts acc   # 99
end
g

# Reverse direction: int seed must shadow an outer string acc without
# the outer binding leaking into the block's `acc + x` (which would
# dispatch as string concat instead of mrb_int addition).
def h
  acc = "outer"
  sum = [1, 2, 3, 4, 5].reduce(0) { |acc, x| acc + x }
  puts sum   # 15
  puts acc   # outer
end
h
