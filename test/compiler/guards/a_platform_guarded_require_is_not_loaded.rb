# A `require` under a platform guard names a file this program never loads, so
# a whole-program compiler must not load it either. mixlib-shellout requires its
# windows half from inside `class ShellOut`, and that half is full of windows
# FFI types -- following the require compiled code written for a platform this
# build is not, and failed on the first `:dword`.
#
# `windows_only.rb` here contains exactly that: a `:dword` field zeo has no type
# for. If the require were followed, this file would not compile at all, which
# is what makes the assertions below more than a `defined?` check.

if RUBY_PLATFORM =~ /mswin|mingw|windows/
  require_relative "a_platform_guarded_require_is_not_loaded/windows_only"
else
  require_relative "a_platform_guarded_require_is_not_loaded/posix_only"
end

p defined?(WINDOWS_MARKER)
p defined?(POSIX_MARKER)
p PosixOnly.kind

# The same guard from inside a class body, which is where the real one sits.
class Host
  if RUBY_PLATFORM =~ /mswin|mingw|windows/
    require_relative "a_platform_guarded_require_is_not_loaded/windows_only"
  end

  def self.kind = :host
end

p Host.kind
p defined?(WINDOWS_MARKER)

# `unless` reads the guard the other way round, and takes the other branch.
unless RUBY_PLATFORM =~ /mswin|mingw|windows/
  POSIX_AGAIN = :yes
else
  require_relative "a_platform_guarded_require_is_not_loaded/windows_only"
end

p POSIX_AGAIN

# `match?` and the reversed operand order ask the same question.
if /mswin|mingw/.match?(RUBY_PLATFORM)
  require_relative "a_platform_guarded_require_is_not_loaded/windows_only"
end

if /mswin|mingw/ =~ RUBY_PLATFORM
  require_relative "a_platform_guarded_require_is_not_loaded/windows_only"
end

p defined?(WINDOWS_MARKER)

# `File::ALT_SEPARATOR` is the oldest windows test there is -- `"\\"` there and
# `nil` everywhere else, so its TRUTH is a property of the build. sys-filesystem
# picks its half with it.
if File::ALT_SEPARATOR
  require_relative "a_platform_guarded_require_is_not_loaded/windows_only"
end

p File::ALT_SEPARATOR
p defined?(WINDOWS_MARKER)

# An engine gate is the same shape, on a different baked constant.
if RUBY_ENGINE =~ /jruby|truffleruby/
  require_relative "a_platform_guarded_require_is_not_loaded/windows_only"
end

p defined?(WINDOWS_MARKER)
p :done
__END__
nil
"constant"
:posix
:host
nil
:yes
nil
nil
nil
nil
:done
