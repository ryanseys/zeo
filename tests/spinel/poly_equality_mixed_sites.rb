def same(a, b)
  a == b
end

puts same(1, 1)
puts same("x", "x")
puts same(1, 1.0)

def less_or_equal(a, b)
  a <= b
end

puts less_or_equal(1, 2.0)
begin
  puts less_or_equal(1, "x")
rescue
  puts false
end
begin
  puts less_or_equal(false, true)
rescue
  puts false
end
begin
  puts less_or_equal(nil, nil)
rescue
  puts false
end

def maybe_string(flag)
  if flag
    "actual"
  else
    nil
  end
end

puts same(maybe_string(false), nil)
puts same(maybe_string(true), nil)
puts same(maybe_string(false), "")
