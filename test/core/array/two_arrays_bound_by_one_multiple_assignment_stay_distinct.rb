# `numbers, strings = [], []` gives two objects, each appended to separately.
# (spinel issue #3213)
def crash
  numbers, strings = [], []
  numbers << 1
  strings << "one"
  [numbers, strings]
end
p crash
__END__
[[1], ["one"]]
