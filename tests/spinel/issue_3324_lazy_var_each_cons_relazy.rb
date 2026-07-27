s = (2..Float::INFINITY).lazy.select { |n| (2...n).none? { |d| n % d == 0 } }
p s.each_cons(2).lazy.first(2)
s2 = (2..30).lazy.select { |n| (2...n).none? { |d| n % d == 0 } }
p s2.each_cons(2).lazy.first(2)
