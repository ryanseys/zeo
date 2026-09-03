# The failing shape ACROSS FILES, which is how bundler and rubygems meet:
# the guarded reopen is a unit walked before the unit holding the real
# bodies.

$LOAD_PATH.unshift(File.expand_path("a_guarded_def_in_an_earlier_unit_does_not_take_a_later_units_position", __dir__))
require_relative "a_guarded_def_in_an_earlier_unit_does_not_take_a_later_units_position/main"
__END__
real
real class method
tagged
