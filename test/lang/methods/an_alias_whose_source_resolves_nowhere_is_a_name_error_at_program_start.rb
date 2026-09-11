# `alias_method :foo, :nope_missing` raises NameError when the class body
# runs, from inside the C method `Module#alias_method`, which the backtrace
# shows as its own frame; nothing after it in the file runs.

class Typo
  alias_method :foo, :nope_missing
end
puts "unreached"
__END__
#@ stderr
lang/methods/an_alias_whose_source_resolves_nowhere_is_a_name_error_at_program_start.rb:6:in 'Module#alias_method': undefined method 'nope_missing' for class 'Typo' (NameError)
	from lang/methods/an_alias_whose_source_resolves_nowhere_is_a_name_error_at_program_start.rb:6:in '<class:Typo>'
	from lang/methods/an_alias_whose_source_resolves_nowhere_is_a_name_error_at_program_start.rb:5:in '<main>'
#@ exit 1
