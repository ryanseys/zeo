# A guard the SOURCE wrote inside a class body asks about the CLASS, and runs
# where it is written -- inside the class-body frame, with `self` bound to the
# class.
#
# Zeo lifts a class body's sole `if` statement back out into the enclosing
# scope, because analyze SYNTHESIZES exactly that shape when it pushes a
# `class X ... end if cond` guard into the body: there the condition was
# written outside, and its locals live outside. A guard the source wrote
# inside the body is indistinguishable by shape, so it was lifted too -- and
# then `self` was the enclosing module rather than the class, so every
# `respond_to?` / `const_defined?` probe asked about the wrong object.
#
# Bundler's `rubygems_ext.rb` is written this way: `class Platform; unless
# respond_to?(:generic)` guards twelve constants that RubyGems already
# defines. Asked of `Gem` instead of `Gem::Platform` the probe answered false,
# so `require "bundler"` redefined all twelve and printed 24 warnings ruby
# does not.

require_relative "a_class_body_guard_asks_about_the_class/ext"

p Gm::Plat::WIDTH
p Gm::Plat::HEIGHT
p Gm::Plat.constants.sort
p Gm::Plat.generic("x")
