r = ((1..Float::INFINITY).lazy.select { |n| n > 2 }.each_cons(2).first(3) rescue $!.class)
p r
p((1..10).lazy.each_cons(3).first(2))
p((1..Float::INFINITY).lazy.select { |n| n.even? }.each_cons(2).first(2))
p((1..8).lazy.select { |n| n > 3 }.each_cons(2).to_a)
