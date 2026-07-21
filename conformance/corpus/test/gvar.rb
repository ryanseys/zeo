# Test global variables
$count = 0
$name = "world"

def increment
  $count = $count + 1
end

def greet
  puts "hello " + $name
end

increment
increment
increment
puts $count
greet
$name = "ruby"
greet

$pi = 3.14
puts $pi
