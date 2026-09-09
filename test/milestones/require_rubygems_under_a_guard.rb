# MILESTONE: `require "rubygems" unless defined?(Gem)` loads RUBYGEMS.
#
# The idiom `bundler/rubygems_ext.rb:3` opens with, and the shape that made
# zeo run a completely different file while answering true.
#
# A guarded require takes the UNIT path where an unguarded one takes the
# splice path, and a unit is keyed in a global table by the spelling that
# demanded it. A `require_relative`'s spelling is RELATIVE, so recording it as
# written put the bare name `rubygems` in that table --  and four files under
# `gems/` are named `rubygems.rb`. pub_grub's `static_package_source.rb` says
# `require_relative 'rubygems'`, so its file claimed the global spelling and
# silently shadowed RubyGems itself: `require "rubygems"` ran
# `Bundler::PubGrub::RubyGems`, returned true, recorded the feature as loaded
# so no later require could recover, and left `Gem` undefined.
#
# Shapes, never versions.

require "rubygems" unless defined?(Gem)

p defined?(Gem)
p Gem::VERSION.is_a?(String)
p Gem::Requirement.default.to_s
p Gem::Version.new("1.2.3").to_s

# NOT `defined?(Bundler)`: the oracle runs with `-rbundler/setup`, so bundler
# is loaded before any program it records starts. The question cannot be asked
# neutrally under it, and the answer recorded here would be the harness's
# rather than the program's.

# ...and a second require answers false, having really loaded the first time.
p require("rubygems")
__END__
"constant"
true
">= 0"
"1.2.3"
false
