def show(label)
  puts format("%-24s %s", label, (yield).inspect)
end

pairs = Array.new(60) { |i| [i % 3, i] }
show("min_by(3)") { pairs.min_by(3) { |x| x[0] } }
show("max_by(3)") { pairs.max_by(3) { |x| x[0] } }
show("min(3)") { pairs.min(3) { |a, b| a[0] <=> b[0] } }
show("max(3)") { pairs.max(3) { |a, b| a[0] <=> b[0] } }
show("min_by(3) enumerator") { pairs.each_with_index.min_by(3) { |x,| x[0] } }
show("min_by(20)") { pairs.min_by(20) { |x| x[0] }.last(4) }
