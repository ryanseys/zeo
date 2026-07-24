puts 1.5 + 2.5
puts 3.0 - 1
puts 2.0 * 3
puts 7.0 / 2
puts 7.5 % 2
puts 2.0 ** 3
puts 1.0 == 1
puts 1.5 < 2.5
puts(1.5 <=> 2.5)
puts(-1.5)
puts 1.0
puts 100.0
puts 3.14

class Adder
  def add(a, b)
    a + b
  end
end

puts Adder.new.add(1, 2.5)
puts Adder.new.add(2.5, 1)

add = ->(x, y) { x + y }
puts add.call(3, 4)
puts add.(3, 4)
puts add[3, 4]

square = lambda { |x| x * x }
puts square.call(5)

incr = -> (n = 1) { n + 1 }
puts incr.call
puts incr.call(10)

class Runner
  def return_test
    f = -> {
      return 10
      20
    }
    puts f.call
    "after"
  end
end

puts Runner.new.return_test

f = ->(x, y) { x + y }
begin
  f.call(1)
rescue ArgumentError => e
  puts "caught: #{e.message}"
end
puts f.call(1, 2)
