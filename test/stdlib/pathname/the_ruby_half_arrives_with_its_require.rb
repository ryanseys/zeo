# ruby has the Pathname class before line 1 (pathname.so), but its find and
# rmtree pair comes from `require "pathname"`.
p defined?(Pathname), Pathname.method_defined?(:find), Pathname.method_defined?(:rmtree)
require "pathname"
p Pathname.method_defined?(:find), Pathname.method_defined?(:rmtree)
p Pathname.new(".").find.class
__END__
"constant"
false
false
true
true
Enumerator
