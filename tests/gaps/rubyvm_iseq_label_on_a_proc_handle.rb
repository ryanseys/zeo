# `#label`/`#base_label` on a PROC-derived `InstructionSequence` handle.
# CRuby names the enclosing frame (`block in <main>`, base `<main>`); a zeo
# Proc carries a source location, not a frame label, so the row refuses.
#
# The rest of `InstructionSequence.of` is already exact -- the handle, its
# class, `path`, `first_lineno` -- see
# `tests/rubyvm_iseq_of_answers_for_a_ruby_callable.rb`. This is the one
# row left, split out of `rubyvm_iseq_serialization.rb` so it can flip on
# its own: that file mixes it with three PERMANENT refusals and could
# therefore never promote.
#
# The fix is a frame label on the proc, which is a third static word on
# every proc construction -- a perf question, not a missing mechanism. A
# `Method`-derived handle already answers, so only the block-derived
# spelling is open.
$stderr.reopen(IO::NULL)

handle = RubyVM::InstructionSequence.of(proc { 1 })
p handle.class
{ label: -> { handle.label }, base_label: -> { handle.base_label } }.each do |name, fn|
  r = begin
    fn.call
  rescue Exception => e
    e.class.to_s
  end
  puts "#{name}\t#{r}"
end

# The method-derived spelling already answers; it is here so a regression
# names which half broke.
def a_method = 1
p RubyVM::InstructionSequence.of(method(:a_method)).label
