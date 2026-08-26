# Two compiled-in files can share a basename, and each `require_relative
# "version"` must load ITS OWN sibling.
#
# `zeo_rt::features::UNITS` is a map, so a unit registered under a bare
# relative spelling is a COLLISION rather than an alias -- one file silently
# answers for the other, the require reports success, and the feature is
# recorded loaded so no later require can recover. That is exactly what
# `pub_grub/static_package_source.rb`'s `require_relative 'rubygems'` did to
# `require "rubygems"`.
#
# A `require_relative` therefore registers its unit under the target's
# ABSOLUTE spelling, which is what the runtime `require_relative` builds
# before it asks. The compiler also warns now when two units claim one
# spelling anyway, and the runtime keeps the FIRST claim so the warning names
# the file that really lost.
#
# Eight files are named `version.rb` across the vendored rubygems and bundler
# trees alone, so this is a shape, not a curiosity.

require_relative "two_files_named_version_each_load_their_own/a/lib"
require_relative "two_files_named_version_each_load_their_own/b/lib"

p load_a_version
p load_b_version
p [A_VERSION, B_VERSION]
