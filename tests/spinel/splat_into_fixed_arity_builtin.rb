# compile side (#3515): the splat reached the slot expecting a single key
k001 = [:a]
p({ a: 1 }.fetch(*k001))

k003 = ["a"]
p({ "a" => 1 }.fetch(*k003))

k004 = [1, :x]
a004 = [1, 2]
a004.insert(*k004)
p a004

k005 = [:b, 2]
h005 = { a: 1 }
h005.store(*k005)
p h005

# runtime side (#3516): it compiled and dropped the extra arguments
k006 = [:b]
h006 = { a: 1 }
h006[*k006] = 9
p h006

k007 = [0, 2]
p([1, 2, 3].slice(*k007))
p([1, 2, 3][*k007])

k008 = [1, 2]
p("hello".slice(*k008))
p("hello"[*k008])

k009 = [5, :d]
p([9].fetch(*k009))

k010 = [0, 2]
a010 = [1, 2, 3]
a010.fill(9, *k010)
p a010

k011 = [1]
p({ 1 => :x }.fetch(*k011))

k012 = ["l", "L"]
p("hello".sub(*k012))

# a literal splat, no local in between
p([1, 2, 3].slice(*[0, 2]))

# the forms that already worked stay working: a user method reads the array
# into its params, and the variadic builtins take it as it stands
def two(a, b)
  a + b
end
k013 = [1, 2]
p two(*k013)
p [1, 2, 3].dig(*[0])
p [1, 2, 3].values_at(*[0, 2])
a014 = [1, 2]
a014.push(*[3, 4])
p a014
