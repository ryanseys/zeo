# A String/Array subclass satisfies the implicit-conversion protocol AS itself
# -- `to_str`/`to_ary` answer the object, not a fresh builtin -- so every site
# that asks for one has to unwrap the payload it carries.
class MyStr < String; end
class MyArr < Array; end
s = MyStr.new("ab")
p "x" + s
p "abcab".gsub(s, "-")
p File.join(s, "c")
a = MyArr.new([1, 2])
p [0].concat(a)
p [[1, 2]].assoc(1)
p Array(a)
p Array(nil)
p Array({a: 1})
p Array(1..3)
p Array(5)
p String(s)
