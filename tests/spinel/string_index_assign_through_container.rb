# `rows[0][1] = "*"` mutates the string the container holds: `[]=` was not a
# container-reaching mutator, so the Array form raised NoMethodError and the
# Hash form silently did nothing (#3940).
rows = [+"abc", +"def"]
rows[0][1] = "*"
rows[1][-1] = "Z"
p rows
rows[0][0, 2] = "XY"
p rows
rows[0][1..2] = "-"
p rows

h = { a: +"abc" }
h[:a][1] = "*"
h[:a][0, 1] = "QQ"
p h

n = { 1 => +"one" }
n[1][0] = "O"
p n

s = String.new("hello")
list = [s]
list[0][0] = "H"
p [s, list]

# the other in-place mutators through a container keep working
rows[1].upcase!
rows[1] << "!"
rows[1].sub!("D", "d")
rows[1].insert(1, ".")
p rows

# a plain local String is unaffected
t = +"abc"
t[1] = "*"
t[0, 1] = "ZZ"
p t

# a nested array element still takes an index assignment
grid = [[1, 2], [3, 4]]
grid[0][1] = 9
p grid
