a001 = (nil.instance_eval { 42 } rescue $!.class); p a001
a002 = (nil.instance_exec(5) { |y| y } rescue $!.class); p a002
v = nil.instance_eval { 42 }; p v
p(nil.instance_exec(7) { |x| x * 2 })
p 5.instance_eval { 10 }
p "hi".instance_exec(3) { |n| n * 2 }
p nil.instance_exec(1, 2) { |a, b| a + b }
p 5.instance_eval { self }
