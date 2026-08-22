# `Kernel#lambda` accepts a block WRITTEN AT THE SITE THAT INVOKES IT and
# refuses one that arrives any other way. zeo accepts every shape.
#
# The rule is a run-time property of the BLOCK HANDLER, not of the block, the
# receiver, or the method name. CRuby's frame carries either an iseq block
# handler (a literal `{ }` at this very call) or a proc handler (a `&expr`, a
# forwarded `&blk` parameter, a `Method#call`), and `rb_block_lambda` refuses
# the second kind. Nothing about the VALUE distinguishes them: the proc a
# refused `lambda(&pr)` hands over was itself built from a literal block.
#
# Two probes below pin that it cannot be answered at compile time either.
# `Kernel.send(m) { 1 }` with a computed `m` is ACCEPTED, and so is
# `"x".send(:lambda) { 1 }` -- `lambda` is a private instance method of
# Kernel, so any receiver reaches it. The compiler can see neither the name
# nor the receiver in general.
#
# So this needs the caller's block ORIGIN to reach the callee: one bit in the
# send ABI, or a per-frame flag a builtin row can read. `Frame` is exactly 40
# bytes with no spare field. That is a hot-path change for one builtin, and it
# is the same shape as `a_later_def_on_a_builtin_reaches_back` -- it wants its
# own pass with a bench number, not a corner of someone else's.
#
# A COMPILE-TIME half is not a partial fix, it is a regression. The receiverless
# literal form is already folded to a Lambda node before any of this
# (`lower/mod.rs`, the `loop`/`block_given?` desugar posture), so making the
# run-time row raise unconditionally would break `Kernel.lambda { 1 }` and
# `Kernel.send(:lambda) { 1 }`, which CRuby accepts. A false raise is worse
# than a false accept.
#
# One CRuby wart the table records: `send` forwards the iseq handler unchanged
# and `public_send` does not, so the two disagree about the same block.
def try(label)
  p [label, yield]
rescue ArgumentError => e
  p [label, :raised, e.message]
end

pr = proc { |a| a }
def wrapper(&b) = lambda(&b)

# Accepted by CRuby: the block is literal at the site that invokes `lambda`.
try(:bare)          { lambda { 1 }.lambda? }
try(:kernel_recv)   { Kernel.lambda { 1 }.lambda? }
try(:send_sym)      { Kernel.send(:lambda) { 1 }.lambda? }
try(:computed_send) { m = :lambda; Kernel.send(m) { 1 }.lambda? }
try(:any_receiver)  { "x".send(:lambda) { 1 }.lambda? }

# Refused by CRuby: the block arrives as a proc handler.
try(:forwarded)     { lambda(&pr).lambda? }
try(:wrapper)       { wrapper { 1 }.lambda? }
try(:method_object) { Kernel.method(:lambda).call { 1 }.lambda? }
try(:public_send)   { Kernel.public_send(:lambda) { 1 }.lambda? }
