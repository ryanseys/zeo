puts [1, [2, 3]] == [1, [2, 3]]
puts({ a: 1, b: [2] } == { a: 1, b: [2] })
puts({ a: 1 } == { a: 2 })

class Blank
end

b1 = Blank.new
b2 = Blank.new
puts b1 == b1
puts b1 == b2
__END__
true
true
false
true
false
