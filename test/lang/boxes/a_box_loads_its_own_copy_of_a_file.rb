#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
# A box requires from its OWN load path, and gets its own copy.
#
# Each box has its own `$LOAD_PATH` (seeded as a copy of the creating
# box's) and its own `$LOADED_FEATURES` (empty), so a file main already
# required RE-EXECUTES in the box -- which is what makes `box::Widget !=
# Widget`, and CRuby's whole point.
#
# The once-only table is keyed by `(box, canonical path)` for exactly that
# reason: the path is the identity CRuby's `$LOADED_FEATURES` uses, and
# the box is what keeps two boxes' copies apart.
#
# An expression-position `box.require` used to be a compile-time
# rejection. A TOP-LEVEL one is still spliced by the loader -- the fast,
# statically typed path -- and everywhere else it is the ordinary send,
# which reaches the box's run-time load path.
root = File.join(__dir__, "a_box_loads_its_own_copy_of_a_file")
$LOAD_PATH.unshift(root)
require "widget"
p BoxedWidget.new.hi

b = Ruby::Box.new
b.load_path.unshift(root)
p b.require("widget")
p b.require("widget")
p b.eval("BoxedWidget.new.hi")
p b.eval("BOXED_WIDGET_CONST")

# Main's copy is untouched: its own class, its own global, its own list.
p [BoxedWidget.new.hi, $boxed_widget_runs]
p b.eval("BoxedWidget") == BoxedWidget
p b.eval("$LOADED_FEATURES.count { |f| f.end_with?(\'widget.rb\') }")
p $LOADED_FEATURES.count { |f| f.end_with?("widget.rb") }

# A second box is a third copy.
c = Ruby::Box.new
c.load_path.unshift(root)
p c.require("widget")
p c.eval("BoxedWidget.new.hi")
p c.eval("BoxedWidget") == b.eval("BoxedWidget")
__END__
"widget 1"
true
false
"widget 1"
99
["widget 1", 1]
false
1
1
true
"widget 1"
false
