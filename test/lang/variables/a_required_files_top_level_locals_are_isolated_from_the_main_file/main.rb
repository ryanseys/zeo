count = 5
require_relative "counterlib"
puts count
puts $lib_count
b = CounterBox.new
b.bump
b.bump
puts b.n
