# `Kernel#lambda` accepts a block WRITTEN AT ITS CALL SITE and refuses one
# that arrives any other way. zeo accepts all five shapes below.
#
# CRuby's rule is about the block's ORIGIN, not its type: `rb_block_lambda`
# asks whether the current block handler came from a literal iseq block, so
# `Kernel.send(:lambda) { 1 }` is fine (the block IS literal at that send)
# while `lambda(&pr)` and `method(:lambda).call { 1 }` are not.
#
# zeo splits the work in two, and the run-time half cannot answer the
# question. The compiler folds a receiverless `lambda { }` with a literal
# block straight to a Lambda node (`lower/mod.rs`, the `loop`/`block_given?`
# desugar posture), which is correct and covers the common case. Everything
# else falls through to the ordinary `Kernel#lambda` row, which takes the
# block through `need_block!` and calls `as_lambda()` on whatever it gets --
# a proc reaching that row carries no record of how it was written.
#
# The fix is to carry the fact the compiler already has. Every call site
# knows which channel it opened: `BlockChannel::Literal` builds the proc
# from a written block, `BlockChannel::Ready` converts a `&expr`. Marking
# the proc at construction -- one bit beside `is_lambda` -- lets the row
# refuse exactly what CRuby refuses, and it is the same bit a `send` carries
# through unchanged, which is what makes `send_literal` keep working.
def try(label)
  p [label, yield]
rescue ArgumentError => e
  p [label, :raised, e.message]
end

pr = proc { |a| a }
try(:literal)        { lambda { 1 }.lambda? }
try(:forwarded)      { lambda(&pr).lambda? }
try(:send_literal)   { Kernel.send(:lambda) { 1 }.lambda? }
try(:send_forwarded) { Kernel.send(:lambda, &pr).lambda? }
try(:method_object)  { method(:lambda).call { 1 }.lambda? }
