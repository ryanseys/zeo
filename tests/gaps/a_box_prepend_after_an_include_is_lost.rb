b = Ruby::Box.new
b.eval("module BoxMix; def mixed = 'm'; end; class Array; include BoxMix; end")
b.eval("module BoxPre; def size = 99; end; class Array; prepend BoxPre; end")
p b.eval("[1, 2].size")
