# Blockless cycle(n) on a poly array is typed as an Enumerator, but the
# emitter materialized the repeated array instead -- so the consumer read a
# poly array through sp_Enumerator_to_a and the program died.
p([].cycle(2).to_a)
p([1,2].cycle(2).to_a)
p([].cycle(0).to_a)
a = [1,:b]
p a.cycle(2).to_a
p a.cycle(2).first(3)
r = []
a.cycle(2) { |x| r << x }
p r
