h1 = { a: 1 }
h1["x"] = 9
p h1

h2 = { "a" => 1 }
h2[:b] = 2
p h2
p h2.keys

h3 = { 1 => 2 }
h3[:x] = 3
p h3

h4 = { 1 => 2 }
h4["x"] = 3
p h4

h5 = { a: 1 }
h5.store("x", 9)
p h5

h6 = { a: 1 }
h6.update({ "x" => 9 })
p h6

h7 = { a: 1 }
k = "x"
h7[k] = 9
p h7

h8 = Hash.new
h8[:a] = 1
h8["x"] = 9
p h8

h9 = {}
h9[:a] = 1
h9["x"] = 9
p h9

s1 = { a: 1 }
s1[:b] = 2
p s1

s2 = { "a" => 1 }
s2["b"] = 2
p s2

s3 = { a: 1 }
s3[1] = 9
p s3

s4 = { a: 1 }
s4[2.5] = 9
p s4

s5 = { a: 1, "x" => 9 }
p s5
p({ a: 1 }.merge({ "x" => 9 }))
