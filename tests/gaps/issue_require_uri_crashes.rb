# A bare `require "uri"` still crashes at load time, now on `defined?`:
# uri/common.rb guards `remove_const(:Parser)` with `if defined?(::URI::Parser)`,
# and zeo answers that truthy even though nothing has set URI::Parser yet (the
# `const_set` on the next line is its only source). zeo takes a branch ruby
# doesn't and removes a constant that was never defined.
#
# (The previously-recorded blockers -- the two-argument `const_defined?` and
# the missing `Module#remove_const` -- are both fixed.)
require "uri"
puts "loaded ok"
