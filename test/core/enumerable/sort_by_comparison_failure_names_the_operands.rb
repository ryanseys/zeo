# A failed comparison inside `sort_by`/`min_by`/`max_by` names the operand
# classes ("comparison of Float with NaN failed"); zeo says "comparison
# failed" without them. Plain `sort` already names them. (Found by the
# 2026-08-24 probe sweep.)
begin
  [1.0, Float::NAN].sort_by { |x| x }
rescue ArgumentError => e
  puts e.message
end
begin
  [1, "a"].min_by { |x| x }
rescue ArgumentError => e
  puts e.message
end
__END__
comparison of Float with NaN failed
comparison of String with 1 failed
