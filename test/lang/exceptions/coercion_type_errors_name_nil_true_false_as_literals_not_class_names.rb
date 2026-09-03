# CRuby renders nil/true/false in a coercion TypeError as those words, not
# `NilClass`/`TrueClass`/`FalseClass`; every other object uses its class
# name (`Integer`, `Symbol`).

def msg
  yield
rescue TypeError => e
  puts e.message
end
msg { "" + nil }
msg { "" + true }
msg { "" + false }
msg { "" + 1 }
msg { "" + :s }
msg { Float(nil) }
msg { Integer(true) }
# A non-class target phrase ("an exact number") keeps the CLASS name,
# even for nil -- the value-name rule is specific to class targets.
msg { Time.at(nil) }
__END__
no implicit conversion of nil into String
no implicit conversion of true into String
no implicit conversion of false into String
no implicit conversion of Integer into String
no implicit conversion of Symbol into String
can't convert nil into Float
can't convert true into Integer
can't convert NilClass into an exact number
