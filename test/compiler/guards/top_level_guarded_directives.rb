# A guarded `def` or `alias` at top level compiles with the guard deciding at
# runtime -- the directive rides into an Object reopen, where the class-body
# conditional machinery already knows how to register-but-not-promise it.
def maybe_here
  "here"
end if ENV["HOME"]
puts maybe_here

alias missing_alias maybe_here if ENV["ZEO_TEST_NEVER_SET"]
begin
  missing_alias
rescue NameError
  puts "not aliased"
end
__END__
here
not aliased
