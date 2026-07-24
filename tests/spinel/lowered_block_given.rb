# A self-recursive yielding method is emitted as a real C function (the block
# becomes a proc parameter) rather than inlined. `block_given?` inside it must
# reflect whether a block was actually passed -- true on the block-bearing call,
# false when invoked without one -- not a constant.
def gen(n)
  return -1 unless block_given?
  yield n
  gen(n - 1) { |x| yield x } if n > 1
  n
end

gen(3) { |v| puts v }   # block present: yields 3, 2, 1
puts gen(5)             # no block: returns -1
