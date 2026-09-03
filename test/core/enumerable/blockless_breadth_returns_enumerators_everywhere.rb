# The blockless breadth: iteration methods across
# Integer/Range/Hash/String/Array answer real Enumerators (inspect shows
# the captured receiver/method/args; `each` re-invokes the source).

p 5.times.to_a
p 2.upto(5).to_a
p 5.downto(2).inspect
p (1..10).step(3).to_a
p({ a: 1, b: 2 }.each_value.to_a)
p "hey".each_char.to_a
p [3, 1].sort_by
p 1.step(2.0, 0.5).to_a
p 5.then.next
p({ x: 1 }.each.next)
p [1, 2, 3].each_slice(2).to_a
__END__
[0, 1, 2, 3, 4]
[2, 3, 4, 5]
"#<Enumerator: 5:downto(2)>"
[1, 4, 7, 10]
[1, 2]
["h", "e", "y"]
#<Enumerator: [3, 1]:sort_by>
[1.0, 1.5, 2.0]
5
[:x, 1]
[[1, 2], [3]]
