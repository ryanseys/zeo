# ruby's top-level `return` ends the FILE it is written in; reading
# resumes in the file that required it. zeo ends `<main>` instead, so
# everything after the first required file's `return` is lost.
#
# ze0 answers ruby's order: each file that writes one gets a flag, and
# every later run of that file -- and every run spliced inside it --
# is skipped.

puts "main: start"
require_relative "a_top_level_return_ends_only_its_file/stops"
puts "main: after stops"
p defined?(STOPS_MARK)
p defined?(NEVER_REQUIRED)

require_relative "a_top_level_return_ends_only_its_file/runs"
puts "main: after runs"
__END__
main: start
stops: start
main: after stops
"constant"
nil
runs: start
runs: end
main: after runs
