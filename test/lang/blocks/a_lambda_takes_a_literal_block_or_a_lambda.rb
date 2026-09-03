# The widened sweep behind `lambda_requires_a_literal_block`: every route a
# block can reach `Kernel#lambda` by.
#
# Three outcomes, and the middle one is the part a narrow reading misses.
# An iseq handler -- a `{ }` written at the site that invokes `lambda` --
# becomes a lambda. A proc handler that IS ALREADY a lambda comes back
# unchanged, which is why `&lam`, `&method(:m)` and `&:sym` all pass.
# Anything else is refused.
#
# `RProc::is_literal_block` carries the first fact: every emitter-built proc
# sets it, the `&expr` conversion clears it, and `public_send`/`Method#call`
# clear it because both re-wrap the block as a proc handler where `send`
# forwards the iseq handler unchanged.

def try(label)
  p [label, yield]
rescue ArgumentError, LocalJumpError, NoMethodError => e
  p [label, e.class, e.message]
end

pr = proc { |a| a }
lam = ->(a) { a }
def forward(&b) = lambda(&b)
def relay(&b) = b
def m_one(x) = x

try(:no_block)        { lambda }
try(:literal)         { lambda { 1 }.lambda? }
try(:literal_do)      { lambda do 1 end.lambda? }
try(:arrow)           { lam.lambda? }
try(:proc_obj)        { pr.lambda? }
try(:proc_literal)    { proc { 1 }.lambda? }
try(:from_proc)       { lambda(&pr).lambda? }
try(:from_lambda)     { lambda(&lam).lambda? }
try(:from_method)     { lambda(&method(:m_one)).lambda? }
try(:from_symbol)     { lambda(&:upcase).lambda? }
try(:forwarded)       { forward { 1 }.lambda? }
try(:relayed)         { lambda(&relay { 1 }).lambda? }
try(:send_literal)    { Kernel.send(:lambda) { 1 }.lambda? }
try(:send_amp)        { Kernel.send(:lambda, &pr).lambda? }
try(:public_literal)  { Kernel.public_send(:lambda) { 1 }.lambda? }
try(:method_call)     { Kernel.method(:lambda).call { 1 }.lambda? }
try(:instance_exec)   { Object.new.instance_exec { lambda { 1 }.lambda? } }
try(:nested)          { [1].map { lambda { 2 }.lambda? }.first }
try(:in_define)       { (Class.new { define_method(:go) { lambda { 3 }.lambda? } }).new.go }
try(:reuse)           { l = lambda { 1 }; lambda(&l).lambda? }
try(:proc_new)        { Proc.new { 1 }.lambda? }
try(:lambda_in_block) { [1].each { |_| lambda { 4 } }.class }
__END__
[:no_block, ArgumentError, "tried to create Proc object without a block"]
[:literal, true]
[:literal_do, true]
[:arrow, true]
[:proc_obj, false]
[:proc_literal, false]
[:from_proc, ArgumentError, "the lambda method requires a literal block"]
[:from_lambda, true]
[:from_method, true]
[:from_symbol, true]
[:forwarded, ArgumentError, "the lambda method requires a literal block"]
[:relayed, ArgumentError, "the lambda method requires a literal block"]
[:send_literal, true]
[:send_amp, ArgumentError, "the lambda method requires a literal block"]
[:public_literal, ArgumentError, "the lambda method requires a literal block"]
[:method_call, ArgumentError, "the lambda method requires a literal block"]
[:instance_exec, true]
[:nested, true]
[:in_define, true]
[:reuse, true]
[:proc_new, false]
[:lambda_in_block, Array]
