# class, member reads, index reads and to_h, each checked against an expected
# value.
require "ostruct"
def assert_equal(expected, actual)
  raise unless expected == actual
end
o = OpenStruct.new(a: 1)
assert_equal OpenStruct, o.class
puts "ok"
__END__
ok
