# Array#combination(k) with a block iterates each combination
# without materialising a giant intermediate array.

count = 0
[1, 2, 3, 4].combination(2) { |c| count = count + 1 }
puts count

# Body actually sees each sub-array.
out = []
[1, 2, 3].combination(2) { |c| out.push(c[0] + c[1]) }
puts out.inspect
