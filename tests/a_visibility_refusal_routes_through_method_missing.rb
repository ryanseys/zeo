# A call refused for VISIBILITY raises `NoMethodError` directly instead of
# going through `method_missing`, so a user hook never sees it.
#
# CRuby refuses an explicit-receiver call to a private or protected method by
# calling `method_missing` with the private/protected reason
# (`rb_method_call_status` then `rb_vm_call0`'s miss path); the DEFAULT
# `method_missing` is what turns that into "private method 'x' called for ...".
# An override intercepts it first -- which is how a delegator or a proxy
# forwards a name its target keeps private.
#
# zeo's refusal sites call `raise_method_missing` directly
# (`explicit_call_barrier` in `dispatch/caches.rs`, `send_value_public_in` and
# `refined_send_in` in `dispatch/mod.rs`), which BUILDS the error rather than
# dispatching the hook. `method_missing_or_raise` -- the miss path for a name
# nothing defines -- is the shape they should share, and it already exists.
#
# The bug is general: the class side has it too, and the per-object singleton
# side inherits it. All three are pinned here.

def try(label)
  p [label, yield]
rescue NoMethodError, NameError => e
  p [label, :raised, e.message]
end

class K
  private def hidden = :hidden
  protected def guarded = :guarded
  def method_missing(n, *a) = [:mm, n, a]
  def respond_to_missing?(n, p = false) = true
end
try(:cls_private)   { K.new.hidden }
try(:cls_protected) { K.new.guarded }
try(:cls_args)      { K.new.hidden(1, 2) }
try(:cls_public)    { K.new.public_send(:hidden) }

o = Object.new
class << o
  private
  def s = :s
  public
  def method_missing(n, *a) = [:mm, n, a]
end
try(:sing_private)  { o.s }
try(:sing_public)   { o.public_send(:s) }

module MFe
  module_function
  def h = :h
end
e = Object.new.extend(MFe)
def e.method_missing(n, *a) = [:mm, n, a]
try(:ext_private)   { e.h }

# With NO hook the message is unchanged.
class Bare
  private def q = :q
end
try(:bare)          { Bare.new.q }

# A class-level hook catches a private CLASS method the same way.
class CM
  def self.hidden_cm = :hidden_cm
  private_class_method :hidden_cm
  def self.method_missing(n, *a) = [:cmm, n]
end
try(:class_method)  { CM.hidden_cm }
