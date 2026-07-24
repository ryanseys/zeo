# Test miscellaneous features: is_a?, respond_to?, String <<, Array#reduce, upto/downto

# String << (append/concat mutating)
s = "Hello"
s << ", World"
puts s  # Hello, World

# Array#reduce / inject
arr = (1..5).to_a
total = 0
arr.each do |x|
  total += x
end
puts total  # 15

# upto / downto
count = 0
1.upto(5) do |i|
  count += i
end
puts count  # 15

count2 = 0
5.downto(1) do |i|
  count2 += i
end
puts count2  # 15

# abs on negative
puts (-42).abs  # 42

# Multiple return (via array)
def min_max(arr)
  mn = arr[0]
  mx = arr[0]
  arr.each do |x|
    if x < mn
      mn = x
    end
    if x > mx
      mx = x
    end
  end
  puts mn
  puts mx
end

data = (1..10).to_a
min_max(data)  # 1, 10
