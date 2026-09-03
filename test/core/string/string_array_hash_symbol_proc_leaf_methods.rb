p "hello".codepoints
s = "hi\n"; p s.chomp!; p s.chomp!
p "hello".partition("l")
p "hello".rpartition("l")
p "a.b.c".partition(/\./)
p "TestName".delete_prefix("Test")
p "file.rb".delete_suffix(".rb")
c = +"x"; c.clear; p c
p((+"y").frozen?)
p [1, 2].repeated_permutation(2).to_a
p [1, 2].repeated_combination(2).to_a
p [1, 2, 3].intersect?([3, 4])
p [1, 2].chain([3], [4]).to_a
p({ a: 1, b: 2, c: 3 }.slice(:a, :c))
p({ a: 1, b: 2 }.except(:a))
p({ a: 1, b: 2 }.fetch_values(:a, :b))
p({ a: 1, b: [2, 3] }.flatten)
p :hello[1, 3]
p :Hello.casecmp?(:hELLO)
f = ->(x) { x + 1 }
g = ->(x) { x * 2 }
p((f >> g).call(3))
p((f << g).call(3))
__END__
[104, 101, 108, 108, 111]
"hi"
nil
["he", "l", "lo"]
["hel", "l", "o"]
["a", ".", "b.c"]
"Name"
"file"
""
false
[[1, 1], [1, 2], [2, 1], [2, 2]]
[[1, 1], [1, 2], [2, 2]]
true
[1, 2, 3, 4]
{a: 1, c: 3}
{b: 2}
[1, 2]
[:a, 1, :b, [2, 3]]
"ell"
true
8
7
