# `Array#join` on a self-referential array must raise ArgumentError
# ("recursive array join"); zeo's runtime panics with a re-entrant
# container access (`value/collections.rs:174`) and the process ABORTS.
# `inspect`/`to_s`/`flatten` already carry the recursion guard this row
# lacks. (Found by the 2026-08-24 probe sweep.)
a = [1]
a << a
begin
  a.join(",")
rescue ArgumentError => e
  puts e.message
end
