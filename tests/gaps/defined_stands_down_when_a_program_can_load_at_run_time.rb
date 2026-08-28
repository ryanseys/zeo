$LOAD_PATH.unshift(File.join(__dir__, "defined_stands_down_when_a_program_can_load_at_run_time"))
require "latecomer"

p RUNTIME_MARK
p defined?(RUNTIME_MARK)

class Reader
  def mark = defined?(RUNTIME_MARK)
end
p Reader.new.mark
