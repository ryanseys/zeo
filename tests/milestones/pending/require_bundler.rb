# MILESTONE (pending): `require "bundler"` STANDALONE, with nothing requiring
# rubygems first.
#
# `require "rubygems"` then `require "bundler"` already works. This ordering
# does not, and the two failures are different bugs:
#
#   `bundler.rb:3`'s `require_relative "bundler/rubygems_ext"` is unguarded, so
#   that file is SPLICED into the main stream. Its `module Gem` is created
#   during the main walk, where `ClassInfo::unit` is None, so the class is
#   never concealed and `defined?(Gem)` is truthy from boot. The
#   `require "rubygems" unless defined?(Gem)` on its own line 3 therefore
#   SKIPS, rubygems.rb never runs, none of its 22 autoload rows is registered,
#   and specification.rb's class body raises on `Gem::Requirement`.
#
# The general rule: a spliced file publishes its constants at a compile-time
# position earlier than CRuby's, and `unless defined?(X)` reads exactly that.
#
# Shapes, never versions -- see `tests/milestones.rs`.

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
