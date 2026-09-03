z = []
r = "a".upto("c") { |s| z << s }
p z
p r
p "9".upto("11").to_a
p "a".upto("e").to_a
p ("9".."11").to_a
p ("a".."e").to_a
p ("y".."ab").to_a
p ("a"..."d").to_a
w = []
"aa".upto("ac") { |s| w << s }
p w
p 1.upto(3).to_a
q = []
1.upto(3) { |i| q << i }
p q
p "a".upto("e", true).to_a
p "a".upto("e", false).to_a
x = []
"a".upto("d", true) { |s| x << s }
p x
__END__
["a", "b", "c"]
"a"
["9", "10", "11"]
["a", "b", "c", "d", "e"]
["9", "10", "11"]
["a", "b", "c", "d", "e"]
[]
["a", "b", "c"]
["aa", "ab", "ac"]
[1, 2, 3]
[1, 2, 3]
["a", "b", "c", "d"]
["a", "b", "c", "d", "e"]
["a", "b", "c"]
