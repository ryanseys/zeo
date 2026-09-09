# `prepend`, which has to beat the builtin row for the box and leave main's
# alone.
#
# Deliberately its OWN box, not the one above: the box path is what this
# program asks about, and main gets the same pair right on its own.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("module BoxPre; def size = 99; end; class Array; prepend BoxPre; end")
p [1, 2].size
p b.eval("[1, 2].size")
__END__
2
99
