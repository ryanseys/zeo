s = (1..Float::INFINITY).lazy.select { |x| x > 3 }
p s.each_cons(2).first(2)
