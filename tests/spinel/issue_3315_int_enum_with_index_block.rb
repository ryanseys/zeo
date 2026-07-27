p(4.times.with_index(1) { |x, i| x })
acc = []
p(3.upto(5).with_index(10) { |x, i| acc << [x, i] })
p acc
p(5.downto(3).each_with_index { |x, i| acc << [x, i] })
p acc
