# A custom `deconstruct` that answers a non-Array raises TypeError
# ("deconstruct must return Array"); zeo's pattern lowering panics in
# `zeo_rt_pat_array_len` (`capi/patterns.rs:107`, a non-unwinding entry)
# and the process ABORTS. (Found by the 2026-08-24 probe sweep.)
c = Class.new { def deconstruct = :not_array }
begin
  case c.new
  in [x]
    p x
  end
rescue TypeError => e
  puts e.message
end
