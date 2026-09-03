# `include`/`prepend`/`extend` inside `class << obj` on a PER-INSTANCE
# singleton -- rdoc's `class << self; prepend Git; end` written inside a method
# body, spreadsheet's `include Compatibility` inside a `module_eval`, treetop's
# and tins'.
#
# These become `recv.singleton_class.include(M)`, which is not a paraphrase of
# the mixin but the primitive itself: CRuby DEFINES `obj.extend(M)` as
# `rb_include_module(rb_singleton_class(obj), M)`. So the runtime routes a
# mix-in on a minted singleton class straight to the same machinery `extend`
# already uses, rather than splicing ancestry the owner's dispatch never reads
# -- which would have made the mixin vanish silently.
#
# Two limits of that machinery are pre-existing and reachable through plain
# `obj.extend(M)` too; they live in tests/gaps/singleton_mixin_super_and_override.rb.

module Git
  def parse_entries
    "git-entries"
  end
end

module Compat
  def helper
    "compat"
  end
end

class Parser
  def parse_entries
    "plain"
  end

  def switch!
    class << self
      prepend Git
    end
    self
  end

  def mixin!
    class << self
      include Compat
    end
    self
  end
end

a = Parser.new
p a.parse_entries
p a.switch!.parse_entries

# Only THAT object is affected -- the class itself is untouched.
p Parser.new.parse_entries

b = Parser.new
p b.mixin!.helper
p Parser.new.respond_to?(:helper)

# Reflection sees the mixin on the singleton.
p a.singleton_class.include?(Git)

# The same three verbs on a plain object outside any method body.
module Tagged
  def tag
    "tagged"
  end
end

o = Object.new
class << o
  include Tagged
end
p [o.tag, Object.new.respond_to?(:tag)]

# `extend` inside `class << obj` reaches one level further out -- the
# singleton's OWN singleton -- exactly as it does inside `class << self`.
module Maker
  def build_it
    "built"
  end
end

q = Object.new
class << q
  extend Maker
end
p q.singleton_class.build_it
__END__
"plain"
"git-entries"
"plain"
"compat"
false
true
["tagged", false]
"built"
