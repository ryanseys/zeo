# `Kernel#lambda` accepts a block WRITTEN AT THE SITE THAT INVOKES IT and
# refuses one that arrives any other way.
#
# The rule is a run-time property of the BLOCK HANDLER, not of the block, the
# receiver, or the method name. CRuby's frame carries either an iseq block
# handler (a literal `{ }` at this very call) or a proc handler (a `&expr`, a
# forwarded `&blk` parameter, a `Method#call`), and `rb_block_lambda` refuses
# the second kind. Nothing about the VALUE distinguishes them: the proc a
# refused `lambda(&pr)` hands over was itself built from a literal block.
#
# So the mark rides on the HANDLE. `ProcData::literal_block` is set for every
# proc the emitter builds -- each one came from a block written in the source
# -- and cleared where a handle turns into CRuby's proc handler: the `&expr`
# conversion (`zeo_rt_block_arg_to_proc`), `public_send`, and `Method#call`.
# `send` forwards the iseq handler unchanged, which is the CRuby wart that
# makes it and `public_send` disagree about the same block.
#
# The compile-time half could not have answered it. `Kernel.send(m) { 1 }`
# with a computed `m` is accepted, and so is `"x".send(:lambda) { 1 }` --
# `lambda` is a private instance method of Kernel, so any receiver reaches it.
#
# `a_lambda_takes_a_literal_block_or_a_lambda.rb` carries the widened sweep,
# including the third outcome this file does not reach: a proc handler that
# is ALREADY a lambda comes back unchanged.

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
