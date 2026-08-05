# A `define_method` body is a BLOCK, and ruby labels its frame as one --
# `block in <class:Named>` -- because that is literally what it is. zeo labels
# it after the method it installs, so the frame claims a method scope ruby
# never reports:
#
#     class Named; define_method(:dm) { where }; end
#     Named.new.dm      ruby "block in <class:Named>"   zeo "Named#dm"
#
# A real `def` is the opposite and zeo now gets it right (see
# `tests/method_defined_in_a_block_frame_label.rb`): a `def` written inside a
# block still creates an ordinary method, so ruby names the frame after the
# method. The two are the same question asked of the two ways to define one,
# and the answers go opposite ways -- which is what makes it easy to fix one
# and miss the other.
#
# Only the LITERAL-symbol form diverges. A computed name stays an ordinary call
# with an ordinary block (no `DefMethod` node), so it already answers
# correctly -- the last pair below is the control.
#
# The base is what is wrong, not the depth: the anonymous case below already
# counts two levels, and only names `Object#dm2` where ruby names `<main>`.

def where = caller_locations(1, 1).first.label

class Named
  define_method(:dm) { where }
end
p Named.new.dm

Anon = Class.new { define_method(:dm2) { where } }
p Anon.new.dm2

# A computed name never becomes a `DefMethod` node -- already correct.
name = :dm3
Anon2 = Class.new { define_method(name) { where } }
p Anon2.new.dm3

# A real `def` through a block names the method -- already correct.
[1].each { def plain = where }
p plain
