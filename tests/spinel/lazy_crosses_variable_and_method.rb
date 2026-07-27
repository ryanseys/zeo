# a lazy chain held in a local or returned from a method
p [1, 2, 3].lazy.first
p [1, 2, 3].lazy.first(2)
p [1, 2, 3].lazy.to_a
p ["a", "b"].lazy.first
p (1..3).lazy.first

z = [1, 2, 3].lazy
p z.first
y = [1, 2, 3].lazy
p y.map { |i| i * 2 }.first
x = (1..Float::INFINITY).lazy
p x.select { |i| i % 3 == 0 }.first(2)

def lz;  [1, 2, 3].lazy; end
p lz.first
p lz.class
def lzr; (1..3).lazy; end
p lzr.first
def lzm; [1, 2, 3].lazy.map { |i| i * 2 }; end
p lzm.first
p lzm.to_a
