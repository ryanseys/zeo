# Allocation and collection under the linked runtime: the heap survives a
# forced collection and the surviving objects keep their contents.
kept = []
2000.times { |i| kept << "row #{i}" if (i % 200).zero? }
20_000.times { "garbage".dup }
GC.start
p kept
puts kept.size
__END__
["row 0", "row 200", "row 400", "row 600", "row 800", "row 1000", "row 1200", "row 1400", "row 1600", "row 1800"]
10
