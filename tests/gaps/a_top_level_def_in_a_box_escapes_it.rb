# A top-level `def` written in a box is callable from main.
#
# A top-level `def` registers on `Object`, and analyze does that regardless
# of `box_id` -- the `BoxScope` arm of the class/def walk records nested
# `ClassDef`s and has no arm for a bare `def`. So the method lands on the
# shared `Object` rather than on the box's own root, on BOTH paths: the
# literal splice and the run-time snippet.
#
# This is the method-table twin of
# `a_class_written_in_a_box_escapes_it.rb`, and it has the same fix (G7's
# B0): a definition written in a box installs on that box's root.
#
# Oracle: main has no such method, so calling it raises NameError.
b = Ruby::Box.new
b.eval("def leaked_helper = 1")
begin
  p leaked_helper
rescue NameError, NoMethodError => e
  p e.class
end
