# ruby's top-level `return` ends the FILE it is written in; reading
# resumes in the file that required it. zeo ends `<main>` instead, so
# everything after the first required file's `return` is lost.
#
# Each file that writes one gets its own end to jump to, and every later
# run of that file -- and every run spliced inside it -- is skipped.
#
# `crates/zeo-rt/ext/digest/lib/digest.rb:12` opens with a guarded one, and
# stdlib files use the form to bail out early.

puts "main: start"
require_relative "a_top_level_return_ends_only_its_file/stops"
puts "main: after stops"
p defined?(STOPS_MARK)
p defined?(NEVER_REQUIRED)

require_relative "a_top_level_return_ends_only_its_file/runs"
puts "main: after runs"

# It carries through a `require_relative` the returning file writes itself,
# and out of a `begin`/`rescue` around it.
require_relative "a_top_level_return_ends_only_its_file/nested"
puts "main: after nested"
require_relative "a_top_level_return_ends_only_its_file/guarded"
puts "main: after guarded"
__END__
main: start
stops: start
main: after stops
"constant"
nil
runs: start
runs: end
main: after runs
nested: start
nested_dep: ran
main: after nested
guarded: start
main: after guarded
