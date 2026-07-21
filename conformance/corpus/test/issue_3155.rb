require "ostruct"
def assert_equal(expected, actual)
  raise unless expected == actual
end
o = OpenStruct.new(a: 1)
assert_equal OpenStruct, o.class
puts "ok"
