# MILESTONE: `require "bundler"` standalone, with nothing requiring rubygems
# first.
#
# Three independent defects had to go before this matched, and all three were
# silent.
#
# `bundler.rb:3`'s unguarded `require_relative "bundler/rubygems_ext"` used to
# SPLICE that file into the main stream, where its `module Gem` was created
# with `ClassInfo::unit` None and therefore never concealed. `defined?(Gem)`
# was truthy from boot, the `require "rubygems" unless defined?(Gem)` on its
# own line 3 skipped, and specification.rb raised on `Gem::Requirement`.
# Load-faithful packages fixed that: no file of rubygems or bundler is
# spliced, so `module Gem` is created under `unit_walk`, concealed at boot,
# and revealed at its own body site.
#
# Then `Bundler::Source::Git` existed TWICE. `bundler/settings.rb`'s unrelated
# `Path = Struct.new(...) do ... end` matched by BARE LEAF against every
# `class X < Path` in the program, so `class Git < Path` in
# `bundler/source/git.rb` was rewritten into a runtime `Git =
# Class.new(Path)`. The compile-time class kept only the bare reopen in
# `git_proxy.rb` for a body site, was never revealed, and raised on every read
# -- while `const_get(:Git)` answered the runtime one. A constant write is
# keyed by its cref path now, and queried by ruby's own lexical search.
#
# Last, 24 warning lines on stderr that ruby does not print.
# `bundler/rubygems_ext.rb:54`'s `class Platform` body is ONE statement, an
# `unless respond_to?(:generic)` over twelve constant writes -- the same shape
# analyze synthesizes when it pushes a `class X ... end if cond` guard into a
# body, so codegen lifted the condition back out of the class-body function.
# Lifted, it ran with `self` bound to `Gem`, answered false, and redefined all
# twelve. A synthesized guard carries a flag now; a guard the source wrote
# stays where it was written.
#
# Shapes, never versions.

require "bundler"

p defined?(Bundler)
p Bundler::VERSION.is_a?(String)
p Bundler::VERSION.split(".").size >= 2
p Bundler.respond_to?(:setup)
p Bundler.respond_to?(:require)
p Bundler::LockfileParser.is_a?(Class)
p Bundler::Definition.is_a?(Class)
p Bundler::Dsl.is_a?(Class)
p Bundler.gem_version.is_a?(Gem::Version)
p Bundler.gem_version.to_s == Bundler::VERSION
__END__
"constant"
true
true
true
true
true
true
true
true
true
