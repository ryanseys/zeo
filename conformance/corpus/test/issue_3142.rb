def crash(*values)
  result = []
  values.each do
    next unless _1.is_a?(String)
    result << _1.match?(/x/)
  end
  result
end
p crash("axb", 5, "y", "xx")

# return unless form, explicit param
def first_string(*values)
  values.each do |v|
    next unless v.is_a?(String)
    return v.upcase
  end
  nil
end
p first_string(1, 2, "hello", "world")

# break unless
def find_int(arr)
  found = nil
  arr.each do |e|
    break unless e.is_a?(Integer)
    found = e * 2
  end
  found
end
p find_int([10, 20])
p find_int(["x", 5])
