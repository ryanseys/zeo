case 5
in Integer => n
  puts n + 1
end

case [1, 2, 3]
in [Integer => a, *rest]
  puts a
  puts rest.length
end

case [1, 2, 3, 4, 5]
in [*pre, 3, *post]
  puts pre.length
  puts post.length
end

x = 5
case 10
in ^x
  puts "same as x"
else
  puts "different"
end

case 3
in 1 | 2 | 3
  puts "small"
else
  puts "big"
end

class Classifier
  def classify(v)
    case v
    in Integer => n if n % 2 == 0
      "even int #{n}"
    in Integer
      "odd int"
    in String
      "string"
    else
      "other"
    end
  end
end
c = Classifier.new
puts c.classify(4)
puts c.classify(3)
puts c.classify("hi")

h = { a: 1, b: 2, c: 3 }
case h
in { a: Integer => av, **rest }
  puts av
  puts rest.length
end

case { a: 1 }
in { a: 1, **nil }
  puts "exact match"
end

case [1, 2, 3, 4, 5]
in [*, Integer => mid1, Integer => mid2, *]
  puts "#{mid1} #{mid2}"
end

case 5
in 1..10
  puts "in range"
end

class Point
  def initialize(x, y)
    @x = x
    @y = y
  end
  def deconstruct
    [@x, @y]
  end
  def deconstruct_keys(keys)
    { x: @x, y: @y }
  end
end

p1 = Point.new(1, 2)
case p1
in [px, py]
  puts "array: #{px}, #{py}"
end
case p1
in Point(x:, y:)
  puts "constant: #{x}, #{y}"
end

if [1, 2] in [Integer, Integer]
  puts "matched predicate"
end

arr = [1, 2]
arr => [first, second]
puts first
puts second

case nil
in nil
  puts "was nil"
end
