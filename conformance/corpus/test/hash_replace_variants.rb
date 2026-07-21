# Hash#replace on the 4 common typed-hash variants — empty
# receiver and copy entries from `other` in original-key order.
# Returns the receiver.

h1 = {a: 1, b: 2}; h2 = {c: 3, d: 4}; h1.replace(h2)
puts h1.inspect
puts h1[:c]
puts h1[:a]

h3 = {a: "x"}; h4 = {b: "y"}; h3.replace(h4)
puts h3.inspect

h5 = {"a" => 1}; h6 = {"b" => 2}; h5.replace(h6)
puts h5.inspect

h7 = {"a" => "x"}; h8 = {"b" => "y"}; h7.replace(h8)
puts h7.inspect
