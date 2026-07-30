# A gem writes `if RUBY_ENGINE == "truffleruby"` around a whole `class`/`def`
# to pick an engine-specific implementation (prism's serializer does exactly
# this for its StringIO). zeo reports MRI's own identity, so the guard decides
# at compile time and only the CRuby branch is registered -- without the fold,
# the class in the untaken branch reached codegen as a definition the analyze
# walk never registered.
if RUBY_ENGINE == "truffleruby"
  class Fast
    def who = :truffle
  end
else
  Fast = ::String
end
p Fast
p RUBY_ENGINE

if RUBY_ENGINE == "ruby"
  def picked = :mri
else
  def picked = :other
end
p picked

# The negated spelling decides too.
p(RUBY_ENGINE != "jruby")

# The same gate inside a CLASS BODY. Until `branch_has_top_defs` counted a
# `def`, a guard whose branches held only methods never folded: both
# registered, and the one written last won -- so this answered `:other`.
class Engine
  if RUBY_ENGINE == "truffleruby"
    def which = :truffle
  else
    def which = :mri
  end

  if RUBY_ENGINE == "ruby"
    def self.built = :here
  else
    def self.built = :elsewhere
  end
end
p Engine.new.which
p Engine.built

# A guard whose predicate zeo cannot decide keeps BOTH branches and settles it
# at runtime, which is what protects every gate this fold does not reach.
class Dynamic
  if ENV.fetch("ZEO_NOT_SET", "") == "yes"
    def pick = :env
  else
    def pick = :default
  end
end
p Dynamic.new.pick
