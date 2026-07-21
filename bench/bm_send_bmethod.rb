# send_bmethod benchmark (from yjit-bench)
# define_method with static block → compiled as def

define_method(:zero) { 0 }
define_method(:one) { |arg| arg }

total = 0
n = 0
while n < 500000
  zero
  total = total + one(123)
  zero
  total = total + one(123)
  zero
  total = total + one(123)
  zero
  total = total + one(123)
  zero
  total = total + one(123)
  zero
  total = total + one(123)
  zero
  total = total + one(123)
  zero
  total = total + one(123)
  zero
  total = total + one(123)
  n = n + 1
end
puts total
puts "done"
