# Proc#source_location resolves through a local whose sole write is a proc
# literal (#2720). --no-line-map zeroes lines, so assert structure only.
a = ->(x) { x }
p a.source_location.is_a?(Array)
p a.source_location.length
b = proc { 1 }
p b.source_location.is_a?(Array)
