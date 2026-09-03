# Regression guard for the fix above: `ZeroDivisionError` must stay
# `Int`/`Int`-division-specific -- real Ruby's `Float`/`0` is
# `Infinity`/`-Infinity`/`NaN` (IEEE semantics), never a raised
# exception, and `float_div` already gives this for free with no
# check needed.

puts(1.0 / 0.0); puts(-1.0 / 0.0)
__END__
Infinity
-Infinity
