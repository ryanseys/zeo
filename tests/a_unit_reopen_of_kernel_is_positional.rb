# A reopen of a builtin inside a LAZY UNIT is positional too.
#
# A reopen normally gets one `.bss` byte -- `zeo_reopen_flags` -- stored at
# the `class Foo ... end` and read by the reopened body, which forwards to
# the row it replaced while the byte is zero. A unit's file was EXCLUDED
# from that, because a unit that is never required leaves the byte at zero
# for the whole program, and a name the unit ADDS would then answer
# `undefined method` forever.
#
# The cost of the exclusion was the whole reason `require "rubygems"` did
# not work. RubyGems reaches `kernel_require.rb` through `eval
# File.read(path)`, and zeo also compiles that file as a unit because the
# package computes a require. Its `def require` therefore registered at
# BOOT -- before the `module Kernel` body that declares the constant that
# body reads -- so the first require in rubygems.rb raised `uninitialized
# constant RUBYGEMS_ACTIVATION_MONITOR`.
#
# A unit's flag now reads the other way: at zero, forward to the native row
# IF THERE IS ONE (`zeo_rt_native_row_exists`), and otherwise run the body.
# A name the unit adds keeps answering; a name it REPLACES answers natively
# until the unit runs, which is ruby's own order.
#
# Two more things had to move with it. Object's method table holds what a
# module MATERIALIZED onto it, so a `def require` written in `module
# Kernel` arrives at the TOP-LEVEL def path -- which hard-coded "no reopen
# flag". And the guard lives in the TRAMPOLINE, so a direct call jumped
# straight past it: a flagged row stands its direct-call nomination down.
#
# `before` must be the NATIVE require's answer, from a call written above
# the reopen. `after` runs the patched body.

$LOAD_PATH.unshift(File.join(__dir__, "a_unit_reopen_of_kernel_is_positional"))
require "pkg"
puts "monitor\t#{Kernel::MONITOR}"
puts "patched\t#{method(:require).owner}"
