# puppet's shape: the platform test is a USER method whose body is
# `!!File::ALT_SEPARATOR`. The predicate's truth is a property of the build,
# so a top-level guard reading it picks its branch at compile time -- and a
# METHOD-BODY `require_relative` under the same (parse-undecidable) guard
# defers to a gated unit instead of splicing eagerly, so a windows-only file
# full of FFI vocabulary zeo has no types for never blocks the build.
module Platform
  def self.windows?
    !!File::ALT_SEPARATOR
  end
end

if Platform.windows?
  KIND = :windows
else
  KIND = :posix
end
p KIND

module Manager
  def self.root?
    if Platform.windows?
      require_relative "a_user_platform_predicate_folds_and_defers/windows_only"
      WindowsUser.admin?
    else
      :uid_zero
    end
  end
end

p Manager.root?
p defined?(WINDOWS_MARKER)
puts "still running"
