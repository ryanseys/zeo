# sort_by on str_array previously bombed at C compile time:
# `cannot resolve call to 'sort_by' on str_array`, then the
# pointer slot got assigned (mrb_int)0. Explicit block and the
# `&:length` / `&:bytesize` / `&:to_i` symbol-to-proc shortcuts
# are now handled.
puts ["bb", "a", "ccc"].sort_by(&:length).inspect
puts ["bb", "a", "ccc"].sort_by { |s| s.length }.inspect
puts ["10", "1", "100"].sort_by(&:to_i).inspect
puts ["zzz", "a", "bb"].sort_by { |s| s.length }.inspect
# Equal-key elements keep their original order (bubble sort is stable).
puts ["bb", "a", "cc"].sort_by(&:length).inspect
