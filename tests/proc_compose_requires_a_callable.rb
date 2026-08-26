# `Proc#>>`/`#<<` type-check their operand AT COMPOSE TIME ("callable
# object is expected"); zeo builds the composite and would only fail when
# called. (Found by the 2026-08-24 probe sweep.)
begin
  p(->(x) { x } >> 5)
rescue TypeError => e
  puts e.message
end
begin
  p(->(x) { x } << "s")
rescue TypeError => e
  puts e.message
end
