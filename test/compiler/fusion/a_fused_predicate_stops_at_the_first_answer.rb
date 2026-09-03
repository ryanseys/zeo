arr = [1, 2, 3, 4]

p arr.all? { |x| x > 0 }
p arr.all? { |x| x > 2 }
p arr.any? { |x| x > 3 }
p arr.any? { |x| x > 9 }
p arr.none? { |x| x > 9 }
p arr.none? { |x| x > 3 }

# An empty receiver never calls the block, and each kind has its own answer.
p [].all? { false }
p [].any? { true }
p [].none? { true }

# Truthiness, not equality: `nil` is falsy and any object is truthy.
p arr.all? { |x| x }
p arr.any? { |x| nil }
p arr.none? { |x| nil }

# How many elements each one actually reaches.
c = 0
p arr.any? { |x| c += 1; x >= 2 }
p c
c = 0
p arr.all? { |x| c += 1; x <= 2 }
p c
c = 0
p arr.none? { |x| c += 1; x >= 3 }
p c

# `count` counts the truthy block values, and its argument form is a
# different method shape that takes the dynamic row.
p arr.count { |x| x > 2 }
p arr.count { |x| nil }
p [].count { true }
p arr.count(2)
p arr.count

# A pattern argument is also a different shape.
p arr.any?(Integer)
p arr.all?(Integer)
p arr.none?(String)

# The block sees a live view.
live = [1, 2, 3]
p live.count { |x| live.pop; true }
__END__
true
false
true
false
true
false
true
false
true
true
false
true
true
2
false
3
false
3
2
0
0
1
4
true
true
true
2
