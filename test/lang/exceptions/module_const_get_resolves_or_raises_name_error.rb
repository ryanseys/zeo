class Foo; BAR = 42; end
p Foo.const_get(:BAR)
p Foo.const_get("BAR")
p Object.const_get(:Foo)
p((Object.const_get(:MissingXYZ) rescue $!.class))
__END__
42
42
Foo
NameError
