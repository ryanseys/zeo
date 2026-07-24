# Test nullable types (T|nil without full poly overhead)
def find_name(arr, target)
  result = nil
  i = 0
  while i < arr.length
    if arr[i] == target
      result = "found:" + target.to_s
    end
    i = i + 1
  end
  result
end

r = find_name([10, 20, 30], 20)
if r.nil?
  puts "not found"
else
  puts r
end

r2 = find_name([10, 20, 30], 99)
if r2.nil?
  puts "not found"
else
  puts r2
end
