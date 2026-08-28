# A local the BLOCK declares, named by a block or lambda nested in it, is
# reached through a cell. The cell was treated as a capture from the enclosing
# frame: its declaration went to the caller, the block-local reset then
# assigned a `_cell_x` the block's own function did not declare, and the C
# build stopped (#4087). Capturing it was also wrong on its own terms -- every
# invocation would share the outer frame's cell, which is what #3230 fixed for
# the inlined shape.
#
# The block's frame owns it now, allocated in the prologue. That has to be the
# prologue rather than the reset, because the reset runs only where emit_stmts
# sees the body as a BlockNode and a proc reached through the poly enumerator
# never gets there -- the body dereferenced a NULL cell.

Name = Struct.new(:params)

box = { "n" => Name.new([1, 2]) }

box["n"].each do |row|
  spelled = []
  row.each { |x| spelled.push("#{x}") }
  puts spelled.join(", ")
end

# per-invocation freshness: each outer iteration gets its own binding
h = { "a" => [1, 2, 3] }
h["a"].each do |v|
  seen = []
  seen.push(v)
  puts seen.join("-")
end

# and each closure keeps the binding it was made with
procs = []
h["a"].each do |v|
  kept = []
  kept.push(v)
  procs.push(proc { kept.join("-") })
end
procs.each { |pr| puts pr.call }

# a name only READ from the enclosing scope is still the enclosing one
outer = []
h["a"].each do |v|
  inner = []
  inner.push(v)
  outer.push(inner.length)
end
p outer

# the cell types that are not pointers, through the same shape
h["a"].each do |v|
  n = 0
  [1, 2].each { |x| n += x }
  puts n.to_s
end

h["a"].each do |v|
  f = 0.0
  [1, 2].each { |x| f += x }
  puts f.to_s
end

h["a"].each do |v|
  s = ""
  [1, 2].each { |x| s += x.to_s }
  puts s
end
