# A later `def self.x` installs at its OWN line: a call written above the
# reopen answers the earlier body, as ruby's install-where-it-stands rule
# requires. `module_function` reaches it the same way -- its class-method copy
# is an ordinary `def self.x` by the time analyze is done.
#
# `analyze::redefs` skipped `is_class_method` in both of its history scans, so
# a `(class, name)` pair on the singleton channel was never even a candidate.
# It counts the two channels APART now -- `def x` and `def self.x` are
# different methods, and only same-channel bodies supersede one another -- and
# each candidate carries which channel it is on, through the boot row, the
# spliced install and the emitted body (whose `self` is the CLASS).
#
# NO call-site change was needed, which is what made this smaller than it
# looked: a class-method send already has an explicit receiver and is already
# dynamic, and the overlay outranks the frozen row. What was missing was the
# install -- `runtime_replace_class_method_c`, whose overlay map holds an
# `RProc` where a compiled trampoline is a `ValueFn`, so the fn is wrapped as
# the value-receiver proc every class-method row there already is.
#
# One ordering fix rode with it: materialization copies an inherited
# `def self.x` onto every descendant, so a SUBCLASS answered the flattened
# copy of the final body and lost the parent's timeline entirely. The
# ancestors' overlay is probed before that flat table now, stopping at the
# first ancestor that really defines the name.
#
# The wider sweep is `tests/class_method_redefinition_timeline.rb`; what the
# window still misreports is `tests/gaps/a_redefinition_window_reports_the_last_body.rb`.

class C
  def self.t = "t1"
end
p C.t
class C
  def self.t = "t2"
end
p C.t

module F
  module_function
  def g = "g1"
end
p F.g
module F
  module_function
  def g = "g2"
end
p F.g
__END__
"t1"
"t2"
"g1"
"g2"
