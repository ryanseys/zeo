# `Module#autoload` runs its target at the DECLARATION, where ruby runs it at
# the constant's first READ.
#
# The cause is not the declaration row -- it is what a lazy unit registers.
# `parse::loader` compiles a literal `autoload :C, "feature"` target in as a
# lazy unit and keeps the call, and `builtins::rmodule`'s `autoload` row loads
# that unit right there. Moving the load to the read is not enough on its own:
# a lazy unit's CLASSES and their methods are in the dispatch tables from
# startup, so `AL::Late.hi` answers before the unit has ever run and no miss
# is left for a read hook to catch. Only the constants a unit's BODY assigns
# (`AL::LOADED_AT` below) actually miss. Making autoload lazy therefore means
# holding a lazy unit's class registrations back until its unit runs, not
# adding a hook at the constant read.
#
# Measured cost: this is what blocks entering `tests/bench/rubygems.rb` and
# `tests/bench/bundler.rb` at the umbrella `require "rubygems"`. Both goldens
# name their own pieces instead, and say so in their headers. With the
# declaration eager, `rubygems.rb`'s `autoload :RequestSet` runs
# `request_set/gem_dependency_api.rb` before `rubygems/platform` is required,
# and it dies on `uninitialized constant Gem::Platform`.
$LOAD_PATH.unshift(File.expand_path("autoload_fixture", __dir__))

puts "before the module"
module AL
  puts "  module body, before the autoload"
  autoload :Late, File.expand_path("autoload_fixture/late.rb", __dir__)
  puts "  module body, after the autoload"
end
puts "after the module"

# Ruby has run nothing yet, and says so.
p AL.autoload?(:Late)&.end_with?("late.rb")
p AL.const_defined?(:Late)
p defined?(AL::LOADED_AT)

puts "first read"
p AL::Late.hi
p AL::LOADED_AT
# Once the feature has loaded, ruby answers nil.
p AL.autoload?(:Late)
