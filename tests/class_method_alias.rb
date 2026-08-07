# An alias inside `class << self` whose source is a BUILTIN class method.
#
# `class << self; alias [] new; end` is the `Klass[...]` constructor shorthand,
# and 14 of the cluster's rows are that one line: rack, rack-test, pry, coderay,
# sprockets, warden, omniauth, better_errors and five more rack-* gems.
#
# The instance side already handled this by recording a NAME indirection rather
# than cloning a body -- the source lives in a static builtin table and has no
# body to clone. The singleton side had no such table, so it raised. Now it has
# one, and `rack-test` mattering to railties and sprockets-rails is why.

class Request
  class << self
    alias [] new
  end

  def initialize(env = {})
    @env = env
  end

  attr_reader :env
end

r = Request["PATH_INFO" => "/"]
p r.class
p r.env

# The alias answers reflection too -- it is a name indirection, not a row, so
# `respond_to?` has to be asked about it separately.
p Request.respond_to?(:[])
p Request.respond_to?(:never_defined)

# A subclass reaches it through the ordinary ancestor walk.
class JsonRequest < Request
end

p JsonRequest["a" => 1].class

# A real `def self.[]` still wins over an alias of the same name: the alias is
# probed only once every real class-method lookup has missed.
class Sized
  class << self
    alias [] new
  end

  def self.[](n)
    "explicit #{n}"
  end
end

p Sized[3]

# An alias whose source is a user `def self.x` never needed this path -- it
# clones the body, as it always did -- but it has to keep working beside it.
class Builder
  def self.build(tag)
    "built #{tag}"
  end

  class << self
    alias make build
  end
end

p Builder.make("x")

# An alias of an alias terminalises, so the rewrite cannot chain at run time.
class Chained
  class << self
    alias [] new
    alias create []
  end

  def initialize(v = 1)
    @v = v
  end

  attr_reader :v
end

p Chained.create(9).v
