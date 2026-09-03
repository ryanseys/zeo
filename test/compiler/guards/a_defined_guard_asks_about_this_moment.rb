# `defined?` and its `const_defined?` / `constants.include?` spellings ask
# whether a name is bound YET. A definition further down the program has not
# run, so at the guard the answer is no -- which is exactly what the
# define-it-if-nobody-else-did idiom is written for.

# guard-compat's shape: create the namespace, then fill it in later.
unless Object.const_defined?('Guard')
  module Guard
  end
end
module Guard
  module Compat
    class Plugin
      def tag = "p"
    end
  end
end
p Guard::Compat::Plugin.new.tag
p Guard.instance_of?(Module)

# ...and the same guard once the name IS bound: the branch is skipped.
module Already; end
unless Object.const_defined?('Already')
  module Already
    LATE = :late
  end
end
p defined?(Already::LATE)

unless defined?(Thor)
  class Thor
    def tag = "stub"
  end
end
p Thor.new.tag

# A conjunction of `defined?`s over a constant nothing in the program has:
# the branch never runs.
if defined?(HTTP) && defined?(HTTP::VERSION)
  module Adapter
    NOPE = 1
  end
end
p defined?(Adapter)

# A `const_set` that pins down NEITHER the constant it names nor the module it
# sets it on is evidence about no constant in particular. webmock's
# `@webMockNetHTTP.const_set(c[0], c[1])` is one of these, and reading it as a
# possible `HTTP::VERSION` left every scoped guard below it undecidable.
class Registry; end
into = Registry
[["Alpha", 1]].each { |pair| into.const_set(pair[0], pair[1]) }
p Registry::Alpha

module HTTP; end
if defined?(HTTP::VERSION)
  module Versioned
    NOPE = 1
  end
end
p defined?(Versioned)

# `Module.constants` names the same list at the top level, and a `map` that
# only respells each entry still holds the same names -- andand asks
# `Module.constants.map { |c| c.to_s }.include?('BlankSlate')`.
unless Module.constants.map { |c| c.to_s }.include?('BlankSlate')
  class BlankSlate
    def tag = "blank"
  end
end
p BlankSlate.new.tag

if Module.constants.map(&:to_s).include?('String')
  module Stringy
    SEEN = true
  end
end
p Stringy::SEEN

# `Object.constants.include?` is the same question through the list -- for an
# `Object` receiver its own constants ARE the top-level ones.
if !Object.constants.include?(:Concurrent)
  module Concurrent
    def self.tag = "shim"
  end
end
p Concurrent.tag

module Present; end
if Object.constants.include?(:Present)
  module Present
    SEEN = true
  end
end
p Present::SEEN

# A global nothing in the program assigns is not defined -- lockfile opens
# with exactly this pair, and stamps the global on its last line. That stamp
# is INSIDE the branch the guard controls, so at the guard it has not run:
# same moment rule the constants above follow.
unless(defined?($__lockfile__) or defined?(Lockfile))
  class Lockfile
    def tag = "lock"
  end
  $__lockfile__ = __FILE__
end
p Lockfile.new.tag
p $__lockfile__.nil?

# `defined?` of a LITERAL is the string "expression", so the branch always
# runs. faraday-stack's `if defined?("Faraday::Env")` means this, whatever
# its author had in mind.
if defined?("Faraday::Env")
  module Patch
    APPLIED = true
  end
end
p Patch::APPLIED

# The interpreter installs a handful of constants on `Object` before the first
# line runs, and no `NAME = ...` in the program says so. Scanning for one and
# finding nothing must not read as "not defined": `defined?(RUBY_ENGINE)` is
# "constant", and a gem that gates its whole engine-compat branch on the probe
# takes the branch.
#
# Every name the runtime seeds is listed here, so a name added to one side and
# not the other shows up as a divergence rather than as a quiet nil.
%w[ARGF ARGV CROSS_COMPILING ENV RUBY_COPYRIGHT RUBY_DESCRIPTION RUBY_ENGINE
   RUBY_ENGINE_VERSION RUBY_PATCHLEVEL RUBY_PLATFORM RUBY_RELEASE_DATE
   RUBY_REVISION RUBY_VERSION STDERR STDIN STDOUT].each do |name|
  p [name, Object.const_defined?(name)]
end

# The mkmf gate itself: defined, and nil, before any require -- CRuby sets
# it at the VM level. A fold that scans the program for `CROSS_COMPILING =`
# finds nothing and must still answer "constant".
p defined?(CROSS_COMPILING)
p CROSS_COMPILING

p defined?(RUBY_ENGINE)
p defined?(RUBY_VERSION)
p defined?(RUBY_PLATFORM)
p defined?(ENV)
p defined?(ARGV)
p defined?(STDOUT)
p defined?(ARGF)
p defined?(STDIN)
p defined?(STDERR)
p defined?(RUBY_PATCHLEVEL)
p defined?(RUBY_REVISION)
p defined?(RUBY_RELEASE_DATE)
p defined?(RUBY_DESCRIPTION)
p defined?(RUBY_COPYRIGHT)
p defined?(RUBY_ENGINE_VERSION)
p defined?(::RUBY_ENGINE)

if defined?(RUBY_ENGINE)
  module Engine
    KNOWN = true
  end
end
p Engine::KNOWN

# The scope OPERATOR does not reach `Object`'s constants, so asking a real
# namespace for one is nil -- the same rule every other qualified read follows.
module Elsewhere; end
p defined?(Elsewhere::RUBY_ENGINE)
__END__
"p"
true
nil
"stub"
nil
1
nil
"blank"
true
"shim"
true
"lock"
false
true
["ARGF", true]
["ARGV", true]
["CROSS_COMPILING", true]
["ENV", true]
["RUBY_COPYRIGHT", true]
["RUBY_DESCRIPTION", true]
["RUBY_ENGINE", true]
["RUBY_ENGINE_VERSION", true]
["RUBY_PATCHLEVEL", true]
["RUBY_PLATFORM", true]
["RUBY_RELEASE_DATE", true]
["RUBY_REVISION", true]
["RUBY_VERSION", true]
["STDERR", true]
["STDIN", true]
["STDOUT", true]
"constant"
nil
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
"constant"
true
nil
