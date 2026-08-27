# A `class ::Module; def method_added` in a file nothing has required yet is
# not installed, so nothing announces itself to it.
#
# Zeo's def-hook pass took a whole-program view: seeing that SOME file defines
# `method_added` on `Module`, it spliced an announcement beside every `def` in
# the program. When the file that defines it is a lazily-loaded feature unit,
# that announcement runs before the hook exists -- and because the same file
# `undef`s the default row first, the send raised `undefined method
# 'method_added'` rather than reaching a no-op.
#
# rake carries this shape: `rake/application.rb` handles `--debugger` with a
# method-body `require "debug/session"`, and debug's session file writes
# `class ::Module; undef method_added; def method_added mid; end`. Merely
# COMPILING rake made `require "rake"` die inside `fileutils`'s module body.
#
# `Module.private_method_defined?(:method_added)` is deliberately NOT printed
# here: the unit's `undef` is still applied at boot, which is its own gap
# (`tests/gaps/a_units_undef_of_a_builtin_row_waits_for_the_unit.rb`).

if ARGV.include?("--debug")
  require_relative "a_units_module_hook_does_not_announce/tracer"
end

module Shop
  def self.buy; end
  def sell; end
end

class Ledger
  def post; end
  def self.open; end
end

p Shop.respond_to?(:buy)
p Shop.instance_methods(false)
p Ledger.new.respond_to?(:post)
p Ledger.respond_to?(:open)
p defined?(Debugger)
