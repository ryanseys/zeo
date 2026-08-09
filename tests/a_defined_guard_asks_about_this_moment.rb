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
# with exactly this pair.
unless(defined?($__lockfile__) or defined?(Lockfile))
  class Lockfile
    def tag = "lock"
  end
end
p Lockfile.new.tag

# `defined?` of a LITERAL is the string "expression", so the branch always
# runs. faraday-stack's `if defined?("Faraday::Env")` means this, whatever
# its author had in mind.
if defined?("Faraday::Env")
  module Patch
    APPLIED = true
  end
end
p Patch::APPLIED
