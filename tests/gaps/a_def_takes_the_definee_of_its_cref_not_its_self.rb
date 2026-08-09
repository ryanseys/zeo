# Ruby's default definee is a property of the FRAME, inherited by every block
# the frame opens -- it is not `self`. `module_eval`/`class_eval` replace it
# with the module, `instance_eval`/`instance_exec` with the receiver's
# singleton, and an ordinary block changes it not at all.
#
# zeo derives it from `self` instead (`define_in_default_definee` takes the
# runtime receiver), which is right for the two `*_eval` forms and for
# `Class.new { }`, and wrong wherever `self` is not the definee:
#
#   a block at the top level  -- self is `main`, definee is Object
#   a `def` inside a `def`    -- self is the instance, definee is Object
#
# Both come out as singleton methods of whatever object was self, so `Object`
# never gains them.
[1].each { def in_block = "block" }
p Object.private_instance_methods(false).include?(:in_block)
p in_block

def outer
  def nested = "nested"
end
outer
p Object.public_instance_methods(false).include?(:nested)
p Object.new.nested

# The forms zeo already gets right, kept beside them so a fix cannot trade one
# for the other.
Anon = Class.new { def built = "built" }
p Anon.new.built
p Anon.public_instance_methods(false)

o = Object.new
o.instance_eval { def only_mine = "mine" }
p o.only_mine
p o.singleton_methods
p Object.private_instance_methods(false).include?(:only_mine)
