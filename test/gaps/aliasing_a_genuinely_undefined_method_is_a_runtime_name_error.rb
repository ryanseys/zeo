# An `alias`/`alias_method` whose source is defined neither in this body
# nor anywhere in the ancestry COMPILES (the compiler can't distinguish a
# typo from a runtime-table builtin source, `alias_method :dup!, :dup`)
# and raises `NameError` at program start instead -- real Ruby's timing,
# the class body executing. See `mro::resolve_aliases` /
# `zeo_rt::validate_aliases`; the full-message shape is asserted in
# `methods::an_alias_whose_source_resolves_nowhere_is_a_name_error_at_program_start`.

class Foo
  alias bar undefined_method
end
__END__
#@ stderr
gaps/aliasing_a_genuinely_undefined_method_is_a_runtime_name_error.rb:10:in '<class:Foo>': undefined method 'undefined_method' for class 'Foo' (NameError)
	from gaps/aliasing_a_genuinely_undefined_method_is_a_runtime_name_error.rb:9:in '<main>'
#@ exit 1
