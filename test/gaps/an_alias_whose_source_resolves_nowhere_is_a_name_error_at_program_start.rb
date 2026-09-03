# `alias_method :foo, :nope_missing`: real Ruby raises NameError when the
# class body EXECUTES (runtime, not compile time) -- mirrored by
# `validate_aliases` at startup, with CRuby's message.

class Typo
  alias_method :foo, :nope_missing
end
puts "unreached"
__END__
#@ stderr
gaps/an_alias_whose_source_resolves_nowhere_is_a_name_error_at_program_start.rb:6:in 'Module#alias_method': undefined method 'nope_missing' for class 'Typo' (NameError)
	from gaps/an_alias_whose_source_resolves_nowhere_is_a_name_error_at_program_start.rb:6:in '<class:Typo>'
	from gaps/an_alias_whose_source_resolves_nowhere_is_a_name_error_at_program_start.rb:5:in '<main>'
#@ exit 1
