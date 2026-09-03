# `raise UndefinedConst, "msg"` -- CRuby evaluates the constant (and
# raises NameError) before the message is ever consulted.

r = begin
  raise Nope, "msg"
rescue => e
  "#{e.class}: #{e.message}"
end
puts r
__END__
NameError: uninitialized constant Nope
