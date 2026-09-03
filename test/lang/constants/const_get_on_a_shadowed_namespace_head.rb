# `Klass.const_get(:NAME)` on a statically-known class folds at compile time.
# The fold took an already-resolved class and round-tripped it through
# `fq_name` -- an UNANCHORED string -- then re-resolved that string against the
# emit site's own cref chain. Any name shadowing along that chain makes the
# round trip lossy.
#
# appraisal2 3.2.2 is the shape: a module with a same-named class nested inside
# it. Resolving the head segment `Appraisal` from inside `module Appraisal`
# finds the nested CLASS, which has no `Hooks`, so the fold panicked with
# `internal error: unknown class/module 'Appraisal::Hooks' in const_owner_id`.
# The gem's own source is correctly anchored (`::Appraisal::Hooks`); it was
# `fq_name` that dropped the anchor.
module Sample
  # The shadow: `Sample::Sample`, so the head segment resolves to a class that
  # knows nothing about the modules beside it.
  class Sample
  end

  module Hooks
    GEMFILE = "Gemfile"
  end

  class << self
    def gemfile = ::Sample::Hooks.const_get(:GEMFILE)
  end
end

p Sample.gemfile

# The panic was the LUCKY case. When the shadowing class also answers the name,
# nothing raises -- the fold silently resolves against the WRONG owner and
# reads the wrong constant. Same lookup, a miscompile instead of a crash.
module Twin
  class Twin
    module Inner
      VALUE = :from_the_nested_class
    end
  end

  module Inner
    VALUE = :from_the_module
  end

  class << self
    def value = ::Twin::Inner.const_get(:VALUE)
    def nested = ::Twin::Twin::Inner.const_get(:VALUE)
  end
end

p Twin.value
p Twin.nested

# The same read spelled every other way must agree.
p Twin::Inner.const_get(:VALUE)
p Twin::Inner::VALUE
p Twin::Inner.const_defined?(:VALUE)
p Sample::Hooks.const_get(:GEMFILE)

# And an unqualified reference from inside the shadowed namespace still means
# what Ruby says it means.
module Twin
  def self.from_inside = Inner.const_get(:VALUE)
end
p Twin.from_inside
__END__
"Gemfile"
:from_the_module
:from_the_nested_class
:from_the_module
:from_the_module
true
"Gemfile"
:from_the_module
