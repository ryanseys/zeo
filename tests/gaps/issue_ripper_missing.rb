# DECLINED, not pending -- see "Declined (a CRuby internal, not a missing
# binding)" in docs/COMPATIBILITY.md. ripper exposes the reduction event
# stream of CRuby's parse.y; zeo's front end embeds prism, a different parser
# with a different event model, so a binding has nothing to bind to. Matching
# ripper means re-implementing CRuby's grammar actions. A Ruby-level Prism API
# would NOT unblock this -- that is what `irb` waits on, a separate gap.
# The gap stays here because it IS a divergence from ruby.
require "ripper"
p defined?(Ripper)
