x = 1
name = "n"
p({x:, name:})
def kw(a:, b:); [a, b]; end
a = 10
b = 20
p kw(a:, b:)
w = "world"
p :"hello_#{w}"
p :"a#{1 + 1}b"
p :"plain"
p :"a#{1}b".class
__END__
{x: 1, name: "n"}
[10, 20]
:hello_world
:a2b
:plain
Symbol
