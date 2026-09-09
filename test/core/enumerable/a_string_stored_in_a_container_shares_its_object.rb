# Appending to a string after putting it in an array is visible through the
# array, and equal? holds.
# (spinel issue #3227)
s1 = "hi"
arr = [s1]
s1 << "!"
p arr[0]
p arr[0].equal?(s1)
h = { k: s1 }
s1 << "?"
p h[:k]

# consumers of the container-read handle
a = "abc"
box = [a, "z", 1]
a << "def"
puts "#{box[0]}!"
p box[0].upcase
p box[0] + "X"
p box.include?("abcdef")
p box.join("-")
box.each { |x| puts x }
p box[0].length
p box[0] == "abcdef"
p box[0] == a
p box[0].class

# in-place append THROUGH the container read
box[0] << "g"
p a

# hash value + string-keyed hash
v = "x"
hs = { "k" => v }
v << "y"
hs.each { |k, w| puts "#{k}=#{w}" }

# push-store shape + sort/mul/cmp via the handle
seed = "ab"
acc = []
acc << seed
seed << "c"
p acc[0]
p ["b", seed, "a"].sort
p seed * 2
p seed <=> "abd"
__END__
"hi!"
true
"hi!?"
abcdef!
"ABCDEF"
"abcdefX"
true
"abcdef-z-1"
abcdef
z
1
6
true
true
String
"abcdefg"
k=xy
"abc"
["a", "abc", "b"]
"abcabc"
-1
