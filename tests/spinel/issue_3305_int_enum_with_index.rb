p 4.times.with_index(1).to_a
p 3.upto(5).with_index(1).to_a
p 5.downto(3).with_index(1).to_a
p 4.times.with_index.to_a

acc = []
3.times.with_index(10) { |x, i| acc << [x, i] }
p acc

p 3.times.each_with_index.to_a
p 2.upto(4).with_object([]) { |x, memo| memo << x * 2 }
