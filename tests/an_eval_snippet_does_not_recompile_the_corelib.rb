# An `eval` snippet must NOT compile the corelib in again.
#
# Every eval in zeo is a run-time compile, and the corelib is a prelude
# segment every compile carries (docs/CORELIB.md). A snippet runs inside a
# program that ALREADY has those rows, so compiling them again re-emits and
# re-installs each one at the snippet's position.
#
# That is a hang, not a slow program. `forwardable`'s `def_delegators` is one
# `module_eval` per delegated name, and `tests/spinel/issue_3300_forwardable.rb`
# went from 0.3s to a 60s timeout the moment the corelib landed -- the sample
# was `zeo_rt_eval_class_body` -> `EvalCompiler::eval`, over and over.
#
# It is the same rule `analyze` already applies to a snippet's own definitions:
# an eval registers nothing, because the program whose tables the rows would
# join is already running. The exception prelude is the exception to it -- that
# IS the bootstrap set every compile resolves against.
p nil.to_i
eval("p nil.to_i")
eval("p NilClass.instance_method(:to_i).source_location")
p NilClass.instance_methods(false).size

# The shape that hung: an eval per name, in a loop.
class Deleg
  def self.make(*names)
    names.each { |n| class_eval("def #{n}(*a) = #{n.to_s.size}", __FILE__, __LINE__) }
  end
  make(:a, :bb, :ccc, :dddd)
end
p [Deleg.new.a, Deleg.new.bb, Deleg.new.ccc, Deleg.new.dddd]

require "forwardable"
class Held
  def add(x) = x * 2
end
class Front
  extend Forwardable
  def_delegators :@h, :add
  def initialize = @h = Held.new
end
p Front.new.add(21)
p nil.rationalize
