# adjacent string literals joined by backslash-continuation, each with its own
# interpolation -- parses to nested InterpolatedStringNodes
def svg(px, inner)
  "<a width='#{px}' " \
  "height='#{px}'>#{inner}</a>"
end

# three-way adjacency mixing plain and interpolated parts
def three(a, b)
  "[" \
  "#{a}-#{b}" \
  "]"
end

# adjacency where one fragment is a plain literal (no interpolation)
def tail(n)
  "n=#{n}" \
  " done"
end

puts svg(16, "x")
puts three("L", "R")
puts tail(7)
