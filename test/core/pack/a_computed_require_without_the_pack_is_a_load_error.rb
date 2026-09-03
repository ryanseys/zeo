# Without the flag the same program raises the LoadError it always did --
# a hermetic binary is the default, not an accident.

require_relative "a_computed_require_without_the_pack_is_a_load_error/prog"
__END__
cannot load such file -- greeter
