# each_slice / each_cons / zip / cycle with a block, on a receiver only known
# to be a container at run time. The blockless forms re-dispatched as the array
# ones; the block forms had no arm and fell to the loud NoMethodError.
rows = [[1, 2, 4, 5], { "a" => 1, "b" => 2 }, "x"]

a = rows[0]
p a.each_slice(2) { |s| s }
p a.each_cons(2) { |s| s }
p a.zip([9]) { |s| s }
p a.cycle(1) { |s| s }

# each_slice and each_cons answer the RECEIVER, so a Hash answers itself and
# not the pairs the re-dispatch materializes from it. zip and cycle answer nil.
h = rows[1]
p h.each_slice(1) { |s| s }
p h.each_cons(1) { |s| s }
p h.zip([1]) { |s| s }
p h.cycle(1) { |s| s }

# the blockless forms still materialize
p a.each_slice(2).to_a
p a.each_cons(2).to_a

# and the guarded forms answer nil
def pick(n) = n > 0 ? [1, 2, 4, 5] : nil
[1, 0].each do |k|
  v = pick(k)
  p v&.zip([9]) { |s| s }
  p v&.cycle(1) { |s| s }
end
