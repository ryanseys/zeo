# The value-consuming fused kinds keep a heap accumulator (map/select's
# result array, sum's generic lane, inject's running value) in a slot the
# epilogue releases. A `break` abandons the half-built accumulator; a
# raise unwinds past the loop entirely; a site inside an enclosing loop
# re-enters with the abandoned value still in the slot. Each shape must
# free exactly once -- the leakcheck leg is the real assertion here.

arr = [3, 1, 4, 1, 5]

# break mid-map answers the break value; the half-built array is freed.
p(arr.map { |e| break :stopped if e == 4; e.to_s })

# ...and re-entering the same site (an enclosing loop) must release the
# abandoned accumulator before seeding a fresh one.
2.times do |i|
  r = arr.map { |e| break :inner if e == 4 && i == 0; e * i }
  p r
end

# A raise inside the body unwinds through the landing, which owns the
# release. The rescue proves the program continues cleanly after.
begin
  arr.map { |e| raise "boom #{e}" if e == 5; e }
rescue RuntimeError => ex
  p ex.message
end

# sum's generic lane holds a heap accumulator when the fold leaves
# numbers; a raise from the generic `+` frees it.
begin
  arr.sum { |e| e == 4 ? :sym : e }
rescue TypeError => ex
  puts "TypeError from the generic lane"
end

# inject's accumulator is replaced every iteration; break abandons the
# current one.
p(arr.inject("") { |a, e| break :cut if e == 5; a + e.to_s })

# select/reject build their answer from re-fetched ORIGINAL elements.
p(arr.select { |e| break [] if e == 5; e > 2 })
p(arr.reject { |e| e > 2 })
