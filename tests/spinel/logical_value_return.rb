def assert_equal(expected, actual)
  if expected != actual
    raise "logical value return mismatch"
  end
end

assert_equal(2, 1 && 2)
assert_equal(nil, nil && 2)
assert_equal(nil, false || nil)
assert_equal(false, false && 3)
assert_equal(true, true || nil)
assert_equal(nil, true && nil)
assert_equal(4, false || 4)

puts "ok"
