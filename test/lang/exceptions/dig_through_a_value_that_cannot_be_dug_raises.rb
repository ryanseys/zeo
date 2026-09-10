# Array#dig raises TypeError when an intermediate value does not respond to dig, and answers nil for a miss.
def cls; begin; yield; rescue => e; e.class; end; end
p(cls { [1, [2]].dig(1, 0, 3) })
p [1, [2]].dig(1, 0)
p [1, [2, [3]]].dig(1, 1, 0)
p [[nil]].dig(0, 0, 5)
p [{a: 7}].dig(0, :a)
__END__
TypeError
2
3
nil
7
