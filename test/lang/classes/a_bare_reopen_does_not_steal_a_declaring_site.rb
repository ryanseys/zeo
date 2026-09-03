# One file REOPENS `class Item` bare; another DECLARES it with a superclass.
# The compiler walks units in demand order, so the bare reopen can come first
# -- and the declaration must still be the one that fixes the superclass and
# runs a class body.
#
# `Bundler::Source::Git` is written this way: `source/git/git_proxy.rb`
# reopens `class Git` bare to hold its error classes, and `source/git.rb`
# declares `class Git < Path`. With the bare file walked first, the
# declaration lost its class-body site entirely: the class was registered from
# the reopen, concealed at boot, and no site was ever emitted to reveal it --
# so the constant read raised for a class the program defines, and the
# `autoload` written inside the declaration's body registered on `Object`.
#
# Which of the two files the compiler walks first is a demand-order fact this
# golden cannot pin, so it pins the SHAPE and the answers. The order itself is
# guarded where it can be: `every_bundled_gem_compiles` asserts that every
# positional class in every bundled gem has a body site some statement stream
# actually reaches, and `--dump=classes` names a violation directly -- a class
# whose sites all read `stream=NONE`, or one whose only site is in the file
# that merely reopens it.

require_relative "a_bare_reopen_does_not_steal_a_declaring_site/root"

p Store::Item
p Store::Item.superclass
p Store::Item.new.base
p Store::Item.new.own
p Store::Item::Nested.new.where
p Store::Item.equal?(Store.const_get(:Item))
p defined?(Store::Item)
p Store::Item.ancestors.take(2).map(&:to_s)
__END__
Store::Item
Store::Base
"base"
"own"
"aitem"
true
"constant"
["Store::Item", "Store::Base"]
