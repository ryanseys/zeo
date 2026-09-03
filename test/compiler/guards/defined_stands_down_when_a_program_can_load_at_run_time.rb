$LOAD_PATH.unshift(File.join(__dir__, "defined_stands_down_when_a_program_can_load_at_run_time"))
require "latecomer"

p RUNTIME_MARK
p defined?(RUNTIME_MARK)

class Reader
  def mark = defined?(RUNTIME_MARK)
end
p Reader.new.mark

# A program with a run-time load can gain a constant this compile never
# saw, so "provably absent" is a claim the compiler is not entitled to
# make. These ask at run time instead of folding to nil.
p defined?(NEVER_DEFINED_ANYWHERE)
p defined?(String)
p defined?(Reader)
module Holder; end
p defined?(Holder)
p defined?(Holder::MISSING)
Object.const_set(:SET_AT_RUN_TIME, 1)
p defined?(SET_AT_RUN_TIME)
eval("EVAL_MARK = 2")
p defined?(EVAL_MARK)
p [defined?(RUNTIME_MARK), defined?(NEVER_DEFINED_ANYWHERE)]
__END__
"assigned by a run-time load"
"constant"
"constant"
nil
"constant"
"constant"
"constant"
nil
"constant"
"constant"
["constant", nil]
