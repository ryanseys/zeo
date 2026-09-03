# `alias` and `undef` at TOP LEVEL, where the implicit target is `Object` --
# top-level `def` defines a private method on Object, so the keywords that
# alias or remove one have to reopen the same class. Both were class-body-only
# and reached the generic "unsupported syntax" rejection out there.

def greet
  "hi"
end
alias hello greet
puts hello
puts greet

def doomed
  "here"
end
undef doomed
puts respond_to?(:doomed, true)
begin
  doomed
rescue NameError
  puts "NameError"
end
__END__
hi
hi
false
NameError
