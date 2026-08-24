# A `module_function` name is TWO rows: a private instance method of the
# module, and a public method on the module's own singleton. `Kernel.puts`
# reaches the second.
#
# `send_value_public_in` asked only the instance walk, and for a `Class`
# receiver that walk reads Class/Module's own ancestry -- Kernel included.
# So it found Kernel's PRIVATE instance row and refused a call CRuby allows.
# A class-method row answers the call, so it decides the visibility.

def try(label)
  p [label, yield]
rescue NoMethodError => e
  p [label, :raised, e.message]
end

try(:kernel_format)  { Kernel.public_send(:format, "%d", 7) }
try(:kernel_send)    { Kernel.send(:format, "%d", 7) }
try(:math_sqrt)      { Math.public_send(:sqrt, 4.0) }
try(:class_allocate) { Class.public_send(:allocate).is_a?(Class) }

module MF
  module_function
  def helper = :helped
end
try(:mf_public)      { MF.public_send(:helper) }
try(:mf_owner)       { MF.instance_method(:helper).owner }

class WithPriv
  def self.hidden = :hidden
  private_class_method :hidden
  def self.shown = :shown
end
try(:priv_cm)        { WithPriv.public_send(:hidden) }
try(:pub_cm)         { WithPriv.public_send(:shown) }
try(:priv_via_send)  { WithPriv.send(:hidden) }

module Ext
  def from_module = :from_module
end
class UsesExt
  extend Ext
end
try(:extended)       { UsesExt.public_send(:from_module) }
try(:module_name)    { Comparable.public_send(:name) }
try(:frozen_q)       { Kernel.public_send(:frozen?) }
