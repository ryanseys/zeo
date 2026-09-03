# A gem that gates on the ruby version by SEGMENT rather than by string writes
# the split once, names it, and indexes the name at every guard beneath. Sass
# is the shape:
#
#   RUBY_VERSION = ::RUBY_VERSION.split(".").map {|s| s.to_i}
#   def ruby1_8? = ironruby? || (RUBY_VERSION[0] == 1 && RUBY_VERSION[1] < 9)
#
# Three things have to hold for that to decide at build time: the split-and-map
# has to reduce to a list, an index into the list has to reduce to a number,
# and the module's own `RUBY_VERSION` has to SHADOW the one ruby seeds -- it is
# a different value under the same name, and every read inside the module means
# the shadow.

module Semver
  extend self

  RUBY_VERSION = ::RUBY_VERSION.split(".").map { |s| s.to_i }
  ENGINE = defined?(::RUBY_ENGINE) ? ::RUBY_ENGINE : "ruby"

  # `&:to_i` is the same mapping, spelled short.
  PARTS = "10.2.30".split(".").map(&:to_i)

  def ironruby? = ENGINE == "ironruby"

  def ruby1_8?
    ironruby? || (Semver::RUBY_VERSION[0] == 1 && Semver::RUBY_VERSION[1] < 9)
  end

  def modern? = Semver::RUBY_VERSION[0] >= 4

  # A negative index counts from the end, and the last segment of the pinned
  # version is what it names.
  def patch = Semver::RUBY_VERSION[-1]
end

p Semver::RUBY_VERSION
p Semver::PARTS
p Semver::ENGINE
p Semver.ruby1_8?
p Semver.modern?
p Semver.patch

# The guard decides, so only one of these classes is ever defined -- the same
# reading `defined?` gives.
if Semver.ruby1_8?
  class Scanner
    def which = :legacy
  end
else
  class Scanner
    def which = :modern
  end
end
p Scanner.new.which

# A receiverless call inside the module reaches the same definition, which is
# what lets `ruby1_8?` open with a bare `ironruby?`.
if Semver.ironruby?
  module Iron
    HERE = true
  end
end
p defined?(Iron)

# `split` keeps a leading empty field and drops the trailing ones, and `to_i`
# reads the leading digits and stops -- so a version with a suffix still
# answers by segment.
module Suffixed
  PARTS = "3.4.0.rc1".split(".").map(&:to_i)
end
p Suffixed::PARTS
p Suffixed::PARTS[3]

# An index past the end is nil in ruby, so a guard over it is not a build-time
# question at all -- it raises, exactly as it would under the interpreter.
module Short
  PARTS = "2.1".split(".").map(&:to_i)
end
p Short::PARTS[2]
begin
  Short::PARTS[2] < 5
rescue NoMethodError => e
  p e.class
end
__END__
[4, 0, 6]
[10, 2, 30]
"ruby"
false
true
6
:modern
nil
[3, 4, 0, 0]
0
nil
NoMethodError
