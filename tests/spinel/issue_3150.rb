def str(*flags)
  out = []
  flags.each do
    raise unless _1.is_a?(String)
    out << _1.match?(/x/)
  end
  out
end
p str("axb", "y", "xx")

# fail / abort forms also gate the fall-through
def only_ints(*vals)
  out = []
  vals.each do |v|
    fail "not int" unless v.is_a?(Integer)
    out << v * 2
  end
  out
end
p only_ints(1, 2, 3)
