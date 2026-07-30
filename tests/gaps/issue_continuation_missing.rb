# DECLINED, not pending -- see "Declined (a CRuby internal, not a missing
# binding)" in docs/COMPATIBILITY.md. `Kernel#callcc` captures and restores the
# machine stack, which a native-compiled program has no runtime for. An
# escape-only callcc is reachable and deliberately not shipped: it answers the
# common upward jump and silently breaks re-entry, which is worse than a
# LoadError a caller can rescue. CRuby itself calls callcc obsolete on load.
# The gap stays here because it IS a divergence from ruby.
require "continuation"
p defined?(Kernel.callcc)
