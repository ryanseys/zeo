# Every name `Object.constants` reports can be written as a constant. CRuby's
# internal `fatal` class carries a name no source can spell, so it is listed
# nowhere and is reachable only as a raised object's class.
p Object.constants.reject { _1.to_s.start_with?(/[A-Z]/) }
p Object.constants.include?(:fatal)
begin
  Object.const_defined?(:fatal)
rescue NameError => e
  puts e.message
end
p Object.constants.all? { _1.to_s.match?(/\A[A-Z]/) }
__END__
[]
false
wrong constant name fatal
true
