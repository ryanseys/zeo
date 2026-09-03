# `size` never iterates: receiver-derived for the same-size set, computed
# for times/upto/each_slice, the stored hint for Enumerator.new, nil when
# unknowable.

p [1, 2, 3].each.size
p [1, 2, 3].select.size
p 5.times.size
p 2.upto(9).size
p [1, 2, 3].each_slice(2).size
p "abc".each_char.size
p Enumerator.new { |y| y << 1 }.size
p Enumerator.new(4) { |y| y << 1 }.size
__END__
3
3
5
8
2
3
nil
4
