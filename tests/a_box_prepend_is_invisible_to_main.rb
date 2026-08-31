b = Ruby::Box.new
b.eval("module BoxPre; def size = 99; end; class Array; prepend BoxPre; end")
p [1,2].size
p b.eval("[1,2].size")
