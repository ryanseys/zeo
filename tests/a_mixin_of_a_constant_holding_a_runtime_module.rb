# A constant the program ASSIGNS a module to, then mixes in. zeo used to
# refuse this outright: a compiled class dispatches off a static MRO, so an
# ancestry edit no compile-time name can describe would vanish silently, and a
# loud refusal beat a wrong answer.
#
# It does not have to vanish. The directive keeps its runtime self-send --
# `runtime_meta::splice_mixin` performs the edit and fires the hook when the
# body executes -- and every call site widens, so nothing folds to the class's
# own body underneath an override the splice installed. Widening EVERY name is
# blunt, but a module minted at run time has no method list to read, and it
# costs the fast path only in programs that do this.
#
# The shape is everywhere. net-ssh picks its `Prompt` out of three candidates
# behind a rescued require; rspec-rails builds `ControllerAssertionDelegator`
# by calling `AssertionDelegator.new(...)`; coderunner's `SYSTEM_MODULE` names
# whichever batch system the host runs.
module Highline
  def prompt(q) = "highline: #{q}"
end

module Clear
  def prompt(q) = "clear: #{q}"
end

Prompt = ENV.key?("ZEO_USES_HIGHLINE") ? Highline : Clear

class KeyFactory
  include Prompt
  def ask = prompt("passphrase")
end

p KeyFactory.new.ask
p KeyFactory.ancestors.first(3).map(&:to_s)
p KeyFactory.new.is_a?(Clear)

# All three directives, against a module with no compile-time identity at all.
# `Loud#speak` reaches the class's own body through `super`, which is the case
# a folded call site would have broken.
Minted = Module.new do
  def tag = "minted"
end

Loud = Module.new do
  def speak = "LOUD(#{super})"
end

Helper = Module.new do
  def build = "built"
end

class Widget
  include Minted
  prepend Loud
  extend Helper
  def speak = "quiet"
end

p Widget.new.tag
p Widget.new.speak
p Widget.build
p Widget.new.respond_to?(:tag)

# Top level: `include M` there is `Object.include(M)`, so the methods reach
# every object.
def delegator(*names)
  Module.new do
    names.each { |n| define_method(n) { "delegated #{n}" } }
  end
end

Delegated = delegator(:alpha, :beta)
include Delegated

p alpha
p Object.include?(Delegated)
