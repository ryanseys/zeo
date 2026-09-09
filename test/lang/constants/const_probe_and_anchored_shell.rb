# Two constant-resolution rules rubygems and bundler lean on all through their
# compatibility shims.
#
# 1. `if defined?(Const)` gates a whole definition. Whether the constant exists
#    is fixed for a whole-program target, so the branch is decided at compile
#    time -- and registration and emission have to decide it the SAME way, or a
#    class reaches codegen with nothing registered behind it.
# 2. A `::`-anchored path names the top level and nothing else, including when
#    the definition it names comes LATER in the flattened program.

# A gate on something the program never defines: the body is unreachable, and
# may name things that don't exist at all.
if defined?(NoSuchThingAnywhere)
  class OnlyUnderTheGate
    def call = NoSuchThingAnywhere::Also::Missing.new
  end
end
p defined?(OnlyUnderTheGate)

# A gate on something the program DOES define: the branch compiles and runs.
class DefinedBelow; end
if defined?(DefinedBelow)
  class GatedOnReal
    VALUE = 41
    def call = VALUE + 1
  end
end
p GatedOnReal.new.call

# The same gate written against a nested path, and against a plain constant
# assignment rather than a class.
module Outer
  module Inner; end
end
p defined?(Outer::Inner)
p defined?(Outer::Nope)
p defined?(NEVER_ASSIGNED)

# A VALUE constant, not a class: `resolve_class` has nothing to say about it,
# so the const registry has to be consulted too.
class Holder
  LIMIT = 7
  def known = defined?(LIMIT)
  def unknown = defined?(NO_SUCH_LIMIT)
end
p Holder.new.known, Holder.new.unknown
module Namespaced
  SIZE = 2
  def self.known = defined?(SIZE)
end
p Namespaced.known
p defined?(Holder::LIMIT)
p defined?(Holder::MISSING)

# ...and `unless defined?` -- the vendored-shim spelling.
unless defined?(Vendored)
  module Vendored
    VERSION = "0.1"
  end
end
p Vendored::VERSION

# A top-anchored superclass whose target is defined later in the program.
module Pkg
  module Timeout
    class Error < RuntimeError; end
  end
end

module Pkg
  module Net
    # Resolved against the top level, skipping the `Pkg::Net` cref -- a bare
    # `Timeout::Error` here would find `Pkg::Timeout::Error` too, but the
    # anchored spelling must not depend on that coincidence.
    class OpenTimeout < ::Pkg::Timeout::Error; end
  end
end

class Elsewhere
  class TimeoutError < ::Pkg::Timeout::Error; end
end

p Pkg::Net::OpenTimeout.ancestors[0, 3]
p Elsewhere::TimeoutError.ancestors[0, 3]
begin
  raise Elsewhere::TimeoutError, "timed out"
rescue Pkg::Timeout::Error => e
  p [e.class, e.message]
end

# A `::`-anchored name that shadows a same-named constant in scope.
COLOUR = :top_level
module Shadow
  COLOUR = :nested
  def self.here = COLOUR
  def self.rooted = ::COLOUR
end
p Shadow.here, Shadow.rooted
__END__
nil
42
"constant"
nil
nil
"constant"
nil
"constant"
"constant"
nil
"0.1"
[Pkg::Net::OpenTimeout, Pkg::Timeout::Error, RuntimeError]
[Elsewhere::TimeoutError, Pkg::Timeout::Error, RuntimeError]
[Elsewhere::TimeoutError, "timed out"]
:nested
:top_level
