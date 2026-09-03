# `prepend`, which has to beat the builtin row for the box and leave main's
# alone.
#
# Deliberately its OWN box, not the one above. An `include` FOLLOWED BY a
# prepend in the same box loses the prepend -- a real divergence, filed as
# `tests/gaps/a_box_prepend_after_an_include_is_lost.rb`. Main gets that
# pair right, so it is the box path specifically.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("module BoxPre; def size = 99; end; class Array; prepend BoxPre; end")
p [1, 2].size
p b.eval("[1, 2].size")
__END__
2
99
