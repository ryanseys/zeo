# A real, previously-undetected bug: `1 / 0` crashed the ENTIRE
# generated binary with a raw Rust panic (`zeo_rt::int_div`'s own
# internal `/` panicking) instead of raising a catchable
# `ZeroDivisionError` -- real Ruby's actual behavior. This affected
# BOTH the static `Int`-`Int` fast path and the runtime-checked
# fallback used for method-parameter operands (always `Poly`). Fixed
# via `emit_int_div_or_mod_checked`, consulted at both call sites.

begin
  puts(5 % 0)
rescue ZeroDivisionError
  puts "mod zero div"
end
class Calc
  def divide(a, b)
    a / b
  end
end
begin
  puts Calc.new.divide(10, 0)
rescue ZeroDivisionError
  puts "param zero div"
end
__END__
mod zero div
param zero div
