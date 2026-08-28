# A local declared in a block that sits inside another block, and captured by a
# third: the cell VARIABLE is declared in the frame that owns the scope, but
# this block body is emitted INSIDE the enclosing block's proc function, where
# the cell is reachable only through the capture struct. The per-iteration
# refresh named the bare `_cell_terms` there, which that function does not
# declare, and the C build stopped (#4127).
Section = Struct.new(:statements)

sections = [Section.new(["Order", "Chaos"])]

sections.each do |section|
  ["a.rb", "b.rb"].each do |path|
    terms = {}
    section.statements.each { |term| terms[term] = path }
    p terms
  end
end

# The refresh is not bookkeeping: it is what gives each iteration its own
# binding. Procs made in the inner block must each see their own `seen`, not
# the last iteration's -- which is the bug the refresh was added for, and the
# reason the fix updates the capture slot rather than skipping the reset.
made = []
[1, 2].each do |outer|
  [10, 20].each do |inner|
    seen = []
    seen << (outer * inner)
    made << proc { seen }
  end
end
p made.map { |pr| pr.call }

# The same shape one level deeper, and with a non-container cell.
counts = []
[[1, 2], [3]].each do |group|
  group.each do |n|
    total = 0
    [n, n].each { |x| total += x }
    counts << proc { total }
  end
end
p counts.map { |pr| pr.call }
