# A user subclass of an IMMEDIATE builtin (D3): the DEFINITION is allowed --
# `superclass`/`ancestors`/`is_a?` resolve -- but there are no instances, so
# `MyInt.new` raises CRuby's exact `NoMethodError`.

class MyInt < Integer; end
puts MyInt.superclass
puts MyInt.ancestors.include?(Integer)
puts MyInt.ancestors.include?(Numeric)
begin
  MyInt.new
rescue => e
  puts "#{e.class}: #{e.message}"
end
class MySym < Symbol; end
begin; MySym.new; rescue => e; puts e.class; end
__END__
Integer
true
true
NoMethodError: undefined method 'new' for class MyInt
NoMethodError
