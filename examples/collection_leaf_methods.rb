# Array, Hash, and Range leaf methods.

# Array#rfind scans from the right; Array#fetch_values is strict.
p [1, 2, 3, 4].rfind { |x| x.even? }
p [1, 2, 3, 4].rfind { |x| x > 9 }
p [10, 20, 30].fetch_values(0, 2)
p([10, 20, 30].fetch_values(0, 5) { |i| i * 100 })
begin
  [10, 20, 30].fetch_values(0, 9)
rescue IndexError => e
  puts e.message
end

# Hash#to_proc turns a hash into a key-lookup callable; transform_keys! mutates.
lookup = { a: 1, b: 2 }.to_proc
p lookup.call(:a)
p [:a, :b].map(&{ a: 10, b: 20 }.to_proc)
h = { a: 1, b: 2 }
p h.transform_keys!(&:to_s)
p h

# Range#overlap? tests whether two ranges share an element.
p (1..5).overlap?(5..8)
p (1...5).overlap?(5..8)
p (1..5).overlap?(6..8)
p (1..5).overlap?(0..0)
p (1..10).overlap?(3..4)

# String#upto walks succ values; the byte* methods index/slice on bytes.
p "a8".upto("b1").to_a
p "hello".byteindex("l")
p "hello".byterindex("l")
p "café".byteslice(0, 3)
p "hello".byteslice(-2, 2)
