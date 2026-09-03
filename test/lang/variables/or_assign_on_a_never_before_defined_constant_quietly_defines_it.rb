# A real, previously-undetected bug: `CONST ||= v` on a constant that
# was NEVER assigned raised `NameError` instead of quietly defining
# it -- confirmed via real `ruby` that this is a genuine, narrow
# special case (ONLY `||=` gets this leniency; `+=`/`&&=` on the same
# undefined constant still raise, see the next test). Fixed via a new
# `HirNode::ConstReadOrNil`, used only by `||=`'s own desugar.

MAX ||= 100
puts MAX
MAX ||= 200
puts MAX
__END__
100
100
