#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
# A `class` written in a box belongs to that box: main cannot reach it.
#
# zeo's boxes share one class registry, and a box's top-level class keeps
# its bare ruby name -- `Escapee` written in a box is called `Escapee`. So
# the registry's name table alone would hand main the box's class, which is
# what `Object.constants` and a bare constant read both scan. A
# `REG_MARK_BOX_CLASS` row records the box each class belongs to, and the
# two scans that answer for a TOP LEVEL -- `Object`'s and a box
# surrogate's -- filter on it. A program with no box emits no such row, so
# the filter is one empty-table read.
#
# Two things this file used to show, both fixed: main could reach the class
# BY NAME, and `defined?` said nil while the read answered -- the two
# disagreeing was the tell, because the fold ran against a table the class
# was never in.

b = Ruby::Box.new
b.eval("class Escapee; def self.hi = 'in box'; end")
p defined?(Escapee)
begin
  p Escapee
rescue NameError => e
  p e.class
end
__END__
nil
NameError
