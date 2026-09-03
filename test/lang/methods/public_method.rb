m = "hello".public_method(:upcase)
p m.call
class C; def foo(x); x*3; end; end
p C.new.public_method(:foo).call(4)
__END__
"HELLO"
12
