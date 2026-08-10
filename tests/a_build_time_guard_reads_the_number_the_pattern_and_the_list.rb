# A version gate is written against the NUMBER as often as against the string
# (`Rails::VERSION::MAJOR`), and a platform gate against a pattern or a list.
# All three are as fixed for a whole-program target as the version strings the
# comparison folders already decide, and each one wraps a whole class in the
# corpus -- so a compiler that cannot decide it registers both arms and reports
# a superclass mismatch for a program ruby runs without complaint.

module Framework
  module VERSION
    MAJOR = 8
    MINOR = 1
    STRING = "8.1.0"
  end
end

if Framework::VERSION::MAJOR == 8 && Framework::VERSION::MINOR >= 1
  class Adapter < Array
    def which = :current
  end
else
  class Adapter < String
    def which = :legacy
  end
end

p Adapter.superclass
p Adapter.new.which

# `String#to_i` reads the leading integer and stops -- chef spells its major
# version this way.
if Framework::VERSION::STRING.to_i >= 12
  class Chef12
    def which = :new
  end
else
  class Chef12
    def which = :old
  end
end
p Chef12.new.which

# An unescaped `.` in a pattern is exactly one character, which is what gmp's
# `unless RUBY_VERSION =~ /^1.8/` says. It reads the same as `/^1\.8/` on
# every real version string, and is matched rather than assumed.
unless RUBY_VERSION =~ /^1.8/
  class Modern
    def which = :modern
  end
end
p Modern.new.which

# `/i` -- the spelling a jruby gate uses.
if RUBY_PLATFORM =~ /java/i
  class Vm < String
    def which = :jvm
  end
else
  class Vm < Array
    def which = :native
  end
end
p Vm.superclass
p Vm.new.which

# `String#[]` hands back the match or nil, so as a CONDITION it asks whether
# the platform string contains that text.
if RUBY_PLATFORM['nosuchplatform']
  class Odd
    def which = :odd
  end
end
p defined?(Odd)

# A literal list asked for membership is the comparison spelled sideways.
if ['opal', 'rubymotion'].include?(RUBY_ENGINE)
  class Web < String
    def which = :web
  end
else
  class Web < Array
    def which = :mri
  end
end
p Web.superclass
p Web.new.which

# `X.instance_methods.include?(:m)` is `X.method_defined?(:m)` the long way --
# a polyfill gate, and it decides against the compiled method tables.
unless Regexp.instance_methods.include?(:match?)
  class MatchPolyfill
    def which = :polyfilled
  end
end
p defined?(MatchPolyfill)

class Bare
  def only_this = 1
end
unless Bare.instance_methods.include?(:absent)
  class Filled
    def which = :filled
  end
end
p Filled.new.which

# `RUBY_PLATFORM.to_s` is `RUBY_PLATFORM` -- `String#to_s` returns self.
if RUBY_PLATFORM.to_s == 'java'
  class Engine
    def which = :jruby
  end
else
  class Engine
    def which = :mri
  end
end
p Engine.new.which
