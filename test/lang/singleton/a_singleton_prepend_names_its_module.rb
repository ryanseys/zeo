# A module prepended into a singleton class WINS the class-method lookup, so
# `super` inside it reaches the class's own `def self.x` and `Method#owner`
# names the module.
#
# zeo flattens a singleton prepend: the module's body becomes the class's
# winning class-method row and the class's own `def self.x` becomes that
# row's super target. Two reflection answers have to say what ruby says in
# spite of the flattening -- `defined?(super)`, which asked the CLASS's
# instance ancestry (`Class`, holding no `def self.x`, so always nil), and
# `#owner`, whose scan correctly finds the host the row sits on.
#
# A module reached through the singleton chain is named BARE, the same rule
# an `extend`ed module already followed: only a real `def self.x` is reported
# as owned by a singleton class.
module SA
  def s = "SA(#{defined?(super) ? super : 'top'})"
end
module SB
  def s = "SB(#{defined?(super) ? super : 'top'})"
end

# The call form.
class Runtime
  def self.s = "Runtime"
end
Runtime.singleton_class.prepend(SB)
p Runtime.s
p Runtime.method(:s).owner.to_s
p Runtime.singleton_class.instance_method(:s).owner.to_s

# The `class << self` form.
class Written
  def self.s = "Written"
  class << self
    prepend SB
  end
end
p Written.s
p Written.method(:s).owner.to_s

# Two modules stack, and the second prepend runs first.
class Stacked
  def self.s = "Stacked"
end
Stacked.singleton_class.prepend(SA)
Stacked.singleton_class.prepend(SB)
p Stacked.s
p Stacked.method(:s).owner.to_s

# An `extend` behind the class's own definition still loses, and its owner
# is reported bare for the same reason.
class Mixed
  extend SA
end
p Mixed.s
p Mixed.method(:s).owner.to_s

# Nothing prepended: the owner is the singleton class itself.
class Plain
  def self.s = "Plain"
end
p Plain.method(:s).owner.to_s
p Plain.singleton_class.instance_method(:s).owner.to_s
__END__
"SB(Runtime)"
"SB"
"SB"
"SB(Written)"
"SB"
"SB(SA(Stacked))"
"SB"
"SA(top)"
"SA"
"#<Class:Plain>"
"#<Class:Plain>"
