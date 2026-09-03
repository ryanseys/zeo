# Ruby's default definee is a property of the FRAME, inherited by every block
# the frame opens -- it is not `self`. `module_eval`/`class_eval` replace it
# with the module, `instance_eval`/`instance_exec` with the receiver's
# singleton, and an ordinary block changes it not at all.
#
# zeo derived it from `self` instead, which is right for those three and wrong
# for every other block: a `def` there came out as a singleton method of
# whatever object happened to be self, so the CREF's class never gained it.
[1].each { def in_block = "block" }
p Object.private_instance_methods(false).include?(:in_block)
p in_block

def outer
  def nested = "nested"
end
outer
p Object.public_instance_methods(false).include?(:nested)
p Object.new.nested

# The cref is where the `def` was WRITTEN, so one inside a method body of a
# class lands on that class -- and is public, since the visibility belongs to
# the frame and only the top level's is private.
class Home
  def build
    def made = "made"
    [1].each { def blocky = "blocky" }
  end
end
Home.new.build
p Home.public_instance_methods(false).sort
p Home.private_instance_methods(false)

module Mod
  def self.go
    [1].each { def from_block = "from_block" }
  end
end
Mod.go
p Mod.instance_methods(false), Mod.singleton_methods(false)

# The three forms that DO replace the definee.
Anon = Class.new { def built = "built" }
p Anon.new.built
p Anon.public_instance_methods(false)

Named = Class.new
Named.class_eval { def evaled = "evaled" }
Named.class_exec { def execed = "execed" }
p Named.public_instance_methods(false).sort
p Object.public_instance_methods(false).include?(:evaled)

o = Object.new
o.instance_eval { def only_mine = "mine" }
p o.only_mine
p o.singleton_methods
p Object.private_instance_methods(false).include?(:only_mine)

# An `instance_eval` nested inside a `class_eval` takes the inner rule, and
# the outer one resumes when it closes.
Named.class_eval do
  o.instance_eval { def nested_only = 1 }
  def after_nesting = 2
end
p o.singleton_methods.sort, Named.public_instance_methods(false).sort
__END__
true
"block"
true
"nested"
[:blocky, :build, :made]
[]
[:from_block]
[:go]
"built"
[:built]
[:evaled, :execed]
false
"mine"
[:only_mine]
false
[:nested_only, :only_mine]
[:after_nesting, :evaled, :execed]
