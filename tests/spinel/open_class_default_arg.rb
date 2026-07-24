class String
  def pad(n = 3)
    self + ("x" * n)
  end

  def bang
    self + "!"
  end
end

class Integer
  def bump(by = 10)
    self + by
  end
end

class TrueClass
  def truthy(x = 7)
    x
  end
end

module Padder
  def wrap(s = "*")
    s + self + s
  end
end

class String
  include Padder
end

# Optional arg omitted -> default filled in.
puts "a".pad
# Optional arg supplied explicitly -> default ignored.
puts "a".pad(1)
# Zero-explicit-param reopen still works (no spurious extra arg).
puts "hi".bang
# Integer reopen, default omitted and supplied.
puts 5.bump
puts 5.bump(2)
# Bool reopen, default omitted and supplied.
puts true.truthy
puts true.truthy(9)
# Module included into a builtin, default omitted and supplied.
puts "in".wrap
puts "in".wrap("-")
