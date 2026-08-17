# A typed array stored into a poly slot -- a Hash value, here -- has a
# different layout from the poly array the index read assumed, so every
# element came back nil.
h = {}
h["a"] = [7, 8, 9]
row = h["a"]
p row[0]
p row[2]
p row[-1]
p row[9]

h["b"] = ["x", "y"]
p h["b"][1]
h["c"] = [1.5, 2.5]
p h["c"][0]
h["d"] = [1, "s"]
p h["d"][1]

# a poly array in the same slot keeps working
h["e"] = [1, :two, "three"]
e = h["e"]
p e[0]
p e[1]
p e[-1]

# and so does a nested read without the intermediate local
p h["a"][1]
