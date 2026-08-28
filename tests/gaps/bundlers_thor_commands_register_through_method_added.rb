# GAP: `Bundler::CLI` finishes with 3 of its ~30 commands, so `zeo bundle
# install` answers `Could not find command "install"` with `install` sitting
# on the class.
#
# Thor registers a command by defining a method and letting `method_added`
# catch it. The three that survive -- config, doctor, plugin -- are the ones
# Thor's `register` adds directly.
#
# THE HALF THAT IS FIXED: `method_added` arrives through the ClassMethods
# idiom (`def self.included(base) = base.extend(ClassMethods)`), and zeo had
# no static edge for that. It has one now
# (`analyze::methods::extends_from_included_hook`), including through a
# reopen, which is the spelling `class Bundler::Thor; include
# Bundler::Thor::Base` actually uses. See
# `tests/a_method_added_hook_arrives_through_extend.rb` and
# `tests/a_method_added_hook_survives_a_reopen.rb`.
#
# THE HALF THAT REMAINS, measured by instrumenting `analyze::def_hooks::fires`
# on 2026-08-28:
#
#     DBG fires? CLI method_added def="cli_help" chain=Some("ClassMethods")
#     DBG  cross-file arm: hook_in_unit=true -> fires=false
#
# `chain=Some("ClassMethods")` says the hook IS found now. What discards it is
# the last arm of `fires`:
#
#     _ => !defined_in_a_unit(compiler, scope, unit_files),
#
# reached when hook and definition sit in different files. The rule reads "a
# hook in a feature unit is installed by a runtime require, so it never saw a
# definition in another file". For an arbitrary pair that is the safe answer.
# It is wrong when the hook's unit is required FIRST, which is the ordinary
# library case: `thor/base.rb` runs, then `bundler/cli.rb` runs, and the hook
# is installed well before CLI's `def`s. zeo has the require graph; `fires`
# only gets a SET of unit filenames, so it cannot tell "another unit" from
# "an earlier unit".
#
# HONESTLY RECORDED: a three-file synthetic of exactly that shape (hook unit
# required first, class defined in a later unit) does NOT reproduce -- zeo
# answers correctly there. So the unit-order rule is necessary to the
# explanation but not sufficient, and something else about bundler's
# structure is also in play. Do not start from the `fires` arm alone; re-probe
# first. See [[zeo-gap-diagnoses-verify]].
#
# Slow on purpose: this compiles the whole bundler graph, which is the only
# place the bug appears.
require "bundler"

p Bundler::CLI.commands.keys.size >= 20
p Bundler::CLI.commands.key?("install")
