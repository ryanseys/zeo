# `<expr>.singleton_class.prepend(M)` where the RECEIVER or the MODULE is not
# a compile-time constant used to be rejected: statically resolved `X.m` call
# sites never consulted the runtime singleton tables the send writes, so the
# prepend would have compiled and then overridden nothing. It now runs as the
# ordinary send it is, and the modules' method names de-optimize those call
# sites so they see the override (`prepend_into_class_singleton`).
module Loud
  def greet = "LOUD " + super
end

module Softer
  def greet = "soft " + super
end

class Speaker
  def self.greet = "hi"
end

# The receiver reaches the class through a LOCAL -- not a constant path.
k = Speaker
k.singleton_class.prepend(Loud)
p Speaker.greet
p Speaker.singleton_class.ancestors.include?(Loud)

# The module reaches the call through an EXPRESSION -- also not a constant.
mods = [Softer]
Speaker.singleton_class.prepend(mods.first)
p Speaker.greet

# A method the modules do NOT define keeps answering the class's own.
def Speaker.name_tag = "speaker"
p Speaker.name_tag
