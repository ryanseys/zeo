# `Box#eval`: a statement-position eval may DEFINE classes in the box; an
# expression-position eval returns a value whose static type is the box's
# own class (Path 1 dispatch on a cross-boundary instance); two boxes
# eval'ing the same class name get distinct classes.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

box = Ruby::Box.new
box.eval("class Gadget; def spin; 'spinning'; end; end")
g = box.eval("Gadget.new")
p g.spin
p box::Gadget.new.spin
box2 = Ruby::Box.new
box2.eval("class Gadget; def spin; 'other'; end; end")
p box2::Gadget.new.spin
p(box::Gadget == box2::Gadget)
p box.eval("1 + 2")
__END__
"spinning"
"spinning"
"other"
false
3
