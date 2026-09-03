# A unit's `class Child < Path` names a superclass another unit defines, and
# the compiler walks the two in demand order -- which is not load order.
#
# `child.rb` lowers before `zpath.rb`, so `Store::Path` is not yet a known
# class definition when `class Child < Path` lowers. That alone is fine: the
# forward-shell machinery covers it. What broke was the OTHER half of the same
# decision -- `settings.rb`'s unrelated `Settings::Path = Struct.new(...) do
# ... end` answered "yes, `Path` is an assigned constant" for the bare leaf,
# across the whole program.
#
# With one half saying "assigned" and the other saying "not a class here", the
# `class` keyword was rewritten into a runtime `Child = Class.new(Path)`. The
# compile-time class then had NO body site at all: it registered, nothing ever
# revealed it, and the constant read raised while `const_get` answered the
# runtime class that had taken its place. Two classes, one name.
#
# This is `bundler/source/git.rb`'s exact shape -- `class Git < Path` with
# `Bundler::Settings::Path` elsewhere in the tree -- and it is what made
# `require "bundler"` die on `Bundler::Source::Git`.

require_relative "a_units_superclass_can_live_in_a_later_unit/settings"
require_relative "a_units_superclass_can_live_in_a_later_unit/root"

p Store::Child
p Store::Child.superclass
p Store::Child.new.hi
p Store::Child.new.kind
# The read and the reflective lookup must find the SAME class.
p Store::Child.equal?(Store.const_get(:Child))
p Store::Path.equal?(Store.const_get(:Path))
p defined?(Store::Child)
p Settings::Path.new(1, 2).kind
__END__
Store::Child
Store::Path
"hi"
"class"
true
true
"constant"
"struct"
