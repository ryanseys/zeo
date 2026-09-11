# An `alias` whose source is defined neither in this body nor anywhere in
# the ancestry compiles, since the compiler cannot tell a typo from a
# builtin source (`alias_method :dup!, :dup`). It raises NameError where
# the statement stands, while the class body runs: the backtrace names
# `<class:Foo>` at the alias line, then `<main>` at the `class` line.
#
#

class Foo
  alias bar undefined_method
end
__END__
#@ stderr
lang/methods/aliasing_a_genuinely_undefined_method_is_a_runtime_name_error.rb:10:in '<class:Foo>': undefined method 'undefined_method' for class 'Foo' (NameError)
	from lang/methods/aliasing_a_genuinely_undefined_method_is_a_runtime_name_error.rb:9:in '<main>'
#@ exit 1
