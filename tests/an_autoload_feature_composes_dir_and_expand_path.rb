# An `autoload` target's feature text is folded at compile time from three
# forms: a string literal, an interpolation whose computed part is `__dir__`,
# and a whole `File.expand_path("<literal>", __dir__)`. They now COMPOSE --
# `"#{File.expand_path("sub", __dir__)}/target"` is the shape rubygems and
# bundler write, and it was none of the three, so the target was left entirely
# to the run-time row and reading the constant raised `LoadError` for a file
# that is right there.
#
# Reading THROUGH an autoloaded namespace is the second half. Only the fold
# that materializes a Class immediate touched the autoload, so
# `Holder::Composed` alone ran the target and `Holder::Composed::MARK` -- the
# same read one level deeper -- raised `uninitialized constant` for a constant
# the target assigns. A scoped read now touches its SCOPE, which is what CRuby
# does before it looks the leaf up.

module Holder
  autoload :Composed, "#{File.expand_path("an_autoload_feature_composes_dir_and_expand_path/sub", __dir__)}/target"
end

puts "before"
p Holder.autoload?(:Composed) != nil
# The NESTED read, which is what has to run the target: nothing has read
# `Holder::Composed` on its own.
p Holder::Composed::MARK
p Holder.autoload?(:Composed) != nil
p Holder::Composed.name

module Sibling
  autoload :Also, "#{File.expand_path("an_autoload_feature_composes_dir_and_expand_path/sub", __dir__)}/also"
end
# The bare read of the constant itself, the shape that already worked.
p Sibling::Also.tag
