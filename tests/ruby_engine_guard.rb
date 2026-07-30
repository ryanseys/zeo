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
