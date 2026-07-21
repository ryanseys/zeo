def ms(nums)
  [nums[0], 0, nums.length]
end
best, lo, hi = ms([5, 6, 7, 8])
d = [0, 10, 20, 30, 40]
p d[lo..hi]
sums = [[1, 2], [3, 4]].map { |a| ms(a).first }
p sums
strs = ["a", "b", "c", "d"]
p strs[lo..hi]
p d[lo, 2]
