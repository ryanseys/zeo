# `RUBY_PLATFORM =~ /mswin|mingw|windows/` is the platform gate half the gem
# corpus writes, and it is decidable when the program is compiled: the platform
# is baked in. Deciding it is what keeps a windows-only branch -- full of types
# and calls that only exist there -- out of the build entirely.
#
# Only a pattern that is `|`-separated LITERAL text folds. Matching one of those
# is a substring test, which cannot disagree with a regexp engine. Anything
# carrying real regexp syntax stays undecided and runs normally.

if RUBY_PLATFORM =~ /mswin|mingw|windows/
  class Impl
    def self.kind = :windows
  end
else
  class Impl
    def self.kind = :posix
  end
end

p Impl.kind

# The pattern on the other side asks the same question.
if /mswin|mingw/ =~ RUBY_PLATFORM
  class Other
    def self.kind = :windows
  end
else
  class Other
    def self.kind = :posix
  end
end

p Other.kind

# `match?` is the same test without the capture.
p RUBY_PLATFORM.match?(/darwin|linux/)
p RUBY_ENGINE =~ /jruby|truffleruby/ ? :alt : :mri

# A guard on a single alternative with nothing to split.
if RUBY_ENGINE =~ /ruby/
  class OnMri
    def self.here = :yes
  end
end

p OnMri.here

# A pattern with real regexp syntax does NOT fold -- and still answers
# correctly at run time, which is the point of refusing to approximate it.
p (RUBY_ENGINE =~ /^ru.y$/) == 0
p RUBY_PLATFORM.match?(/[0-9]+/)
p ("mswin32" =~ /mswin\d+/) == 0

# ... nor does one whose flags change what matching means.
p "MSWIN".match?(/mswin/i)
p "MSWIN".match?(/mswin/)

# The VALUE of `=~` is still an index, not the boolean the guard folded to.
p("abc-darwin" =~ /darwin/)
p("abc" =~ /darwin/)
