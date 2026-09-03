# `method_defined?` on an EXCEPTION class asks at run time, because the
# compiler cannot see the rows those classes have.
#
# The exception hierarchy is written in ruby but carries hand-registered
# NATIVE rows (`exception.rs`'s `mark_owned_names`). Neither table the fold
# consults holds them: they are not compiled `def`s, so `method_in_chain`
# misses, and they reach no `ruby_class!` header, so the projected surface
# misses too. The walk then concluded "not found, and fully projected" --
# a confident FALSE about rows the class really has.
#
# The cost was silent and total: `error_highlight`'s core_ext gates its whole
# API on `Exception.method_defined?(:detailed_message)`, so a false answer
# picked the pre-3.2 `to_s` branch of a library that had already loaded.
module Probe
  ANSWER = if Exception.method_defined?(:detailed_message)
    :folded_or_asked_true
  else
    :answered_false
  end
end

p Probe::ANSWER
p Exception.method_defined?(:detailed_message)
p Exception.method_defined?(:full_message)
p Exception.method_defined?(:no_such_row_anywhere)
p NameError.method_defined?(:name)
p NoMethodError.method_defined?(:private_call?)
p StandardError.method_defined?(:backtrace_locations)

# A class body asks in its own scope, which is where the fold used to run.
class Custom < StandardError
  RESULT = [
    method_defined?(:detailed_message),
    method_defined?(:message),
    method_defined?(:nope),
  ]
end
p Custom::RESULT
__END__
:folded_or_asked_true
true
true
false
true
true
true
[true, true, false]
