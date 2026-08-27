# A unit's `undef` of a builtin row must wait for the unit, as its `def` does.
#
# Zeo makes a unit's REDEFINITION of a builtin method positional -- the row
# forwards to the native implementation until the unit runs. An `undef` in the
# same body is not: `ClassInfo::undefined` is applied when the method tables
# are materialized, so the row is gone from the program's first line.
#
# debug's `debug/session.rb` writes `class ::Module; undef method_added; def
# method_added mid; end`, and rake reaches that file through a method-body
# `require "debug/session"`. Merely compiling rake made
# `Module.private_method_defined?(:method_added)` answer false.
if ARGV.include?("--debug")
  require_relative "a_units_undef_of_a_builtin_row_waits_for_the_unit/tracer"
end

p Module.private_method_defined?(:method_added)
p Module.private_instance_methods(false).sort.grep(/method_/)
p Module.new.send(:method_added, :anything)
p BasicObject.private_method_defined?(:singleton_method_added)
