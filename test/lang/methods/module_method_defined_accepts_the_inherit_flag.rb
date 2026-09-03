class Foo; def bar; end; end
p Foo.method_defined?(:bar)
p Foo.method_defined?(:bar, true)
p Foo.method_defined?(:nope, false)
__END__
true
true
false
