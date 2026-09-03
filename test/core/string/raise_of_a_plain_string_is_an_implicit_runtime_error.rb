class Risky
  def check
    raise "plain string error"
  end
end
Risky.new.check
__END__
#@ stderr
core/string/raise_of_a_plain_string_is_an_implicit_runtime_error.rb:3:in 'Risky#check': plain string error (RuntimeError)
	from core/string/raise_of_a_plain_string_is_an_implicit_runtime_error.rb:6:in '<main>'
#@ exit 1
