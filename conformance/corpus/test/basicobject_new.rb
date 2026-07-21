# BasicObject is instantiable (#2658): the same blank instance Object.new
# builds, tagged with its own builtin id.
o = BasicObject.new
p :created
q = Object.new
p q.class
