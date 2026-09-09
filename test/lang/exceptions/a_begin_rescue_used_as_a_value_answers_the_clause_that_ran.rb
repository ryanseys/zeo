# Each of four begin/rescue expressions answers its rescue body, or its own body when nothing raised.
# (spinel issue #3060)
v = (begin; nil.foo; rescue NoMethodError; true; end); p v
w = (begin; nil.foo; rescue NoMethodError; $!.class; end); p w
x = (begin; raise "boom"; rescue => e; e.message; end); p x
y = (begin; 1 + 1; rescue; 0; end); p y
__END__
true
NoMethodError
"boom"
2
