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

def a_method = 1
p RubyVM::InstructionSequence.of(method(:a_method)).label
