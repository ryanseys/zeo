r = []; (..5).reverse_each { |x| break if x < 3; r << x }; p r

p((1..5).reverse_each.to_a)   # => [5, 4, 3, 2, 1]
p(("a".."e").reverse_each.to_a)

acc = []
(...4).reverse_each { |x| break if x < 1; acc << x }
p acc
n = 0
(..10).reverse_each { |x| n += x; break if x <= 8 }
p n
