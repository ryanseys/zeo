# MILESTONE (pending): `require "bundler"` STANDALONE, with nothing requiring
# rubygems first.
#
# The require itself now SUCCEEDS. `bundler.rb:3`'s unguarded
# `require_relative "bundler/rubygems_ext"` used to splice that file into the
# main stream, where its `module Gem` was created with `ClassInfo::unit` None
# and therefore never concealed -- so `defined?(Gem)` was truthy from boot, the
# `require "rubygems" unless defined?(Gem)` on its own line 3 SKIPPED, and
# specification.rb's class body raised on `Gem::Requirement`.
#
# Load-faithful packages fixed that: no file of rubygems or bundler is spliced,
# so `module Gem` is created under `unit_walk`, concealed at boot, and revealed
# at its own body site. The guard fires and rubygems loads. The general rule
# behind the old failure is worth keeping: a spliced file publishes its
# constants at a compile-time position earlier than CRuby's, and
# `unless defined?(X)` reads exactly that.
#
# What is left is one autoload: `lockfile_parser.rb:184` reads
# `Bundler::Source::Git`, whose unit does not run. Everything above that line
# already matches ruby.
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
