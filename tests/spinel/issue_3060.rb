v = (begin; nil.foo; rescue NoMethodError; true; end); p v
w = (begin; nil.foo; rescue NoMethodError; $!.class; end); p w
x = (begin; raise "boom"; rescue => e; e.message; end); p x
y = (begin; 1 + 1; rescue; 0; end); p y
