# KeyError#key, NameError#name/#receiver, NoMethodError#name/#args/#receiver,
# UncaughtThrowError#tag/#value, and Exception#detailed_message -- populated
# both at the raise site (a failed fetch / missing method / const miss /
# uncaught throw) and from an explicit constructor. Byte-verified against
# ruby 4.0.6.

begin; {}.fetch(:sym); rescue KeyError => e; p e.key; end
begin; {}.fetch(42); rescue KeyError => e; p e.key; end
e = NoMethodError.new("msg", :meth, [1, 2]); p e.args; p e.name; p e.message
p NoMethodError.new("m2", :other).args
p NameError.new("nm", :sym).name
p NameError.new("nm").name
begin; "s".no_such; rescue NoMethodError => e; p e.receiver; p e.name; p e.args; end
begin; nil.foo(1); rescue NoMethodError => n; p n.name; end
begin; TypeError.new("m").name; rescue NoMethodError => e; puts e.class; end
begin; Object.const_get(:Nope); rescue NameError => e; p e.name; p e.receiver; end
begin; Nonexistent; rescue NameError => e; p e.name; end
v = begin; throw :y; rescue UncaughtThrowError => e; e.tag; end; p v
w = begin; throw :z, 7; rescue UncaughtThrowError => e; e.value; end; p w
begin; raise "boom"; rescue => e; p e.detailed_message; end
begin; raise RuntimeError; rescue => e; p e.detailed_message; end
__END__
:sym
42
[1, 2]
:meth
"msg"
nil
:sym
nil
"s"
:no_such
[]
:foo
NoMethodError
:Nope
Object
:Nonexistent
:y
7
"boom (RuntimeError)"
"RuntimeError (RuntimeError)"
