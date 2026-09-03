require "yaml"

p Psych::VERSION.class
p Psych.const_defined?(:VERSION)
__END__
String
true
