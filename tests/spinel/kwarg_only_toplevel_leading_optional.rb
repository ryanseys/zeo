# One shape per file: a companion call that supplies the positional would
# immunise this (#3525).
def top(a = nil, k: :default)
  "a=#{a.inspect} k=#{k.inspect}"
end
puts top(k: 1)
