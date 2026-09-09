# `RubyVM::InstructionSequence.of` answers a real handle for a callable
# written in Ruby and `nil` for a C-defined one. It used to answer `nil` for
# EVERYTHING, which is the one row of the YARV surface that was quietly wrong
# rather than loudly absent: a caller using it to ask "was this defined in
# Ruby?" got the wrong answer with no refusal to notice.
#
# The handle carries no bytecode -- there is none -- so `#to_a`, `#to_binary`
# and `#disasm` refuse on it exactly as they do on a compiled one. See
# `test/divergences/rubyvm_iseq_serialization.rb` for the rows that stay refused.

def try(tag)
  puts "#{tag}: #{yield.inspect}"
rescue StandardError, NotImplementedError => e
  puts "#{tag}: #{e.class}"
end

def ruby_method(a, b = 1); a + b; end
PR = proc { 1 }
LA = ->(x) { x }

try("a proc")        { RubyVM::InstructionSequence.of(PR).class }
try("a lambda")      { RubyVM::InstructionSequence.of(LA).class }
try("a ruby method") { RubyVM::InstructionSequence.of(method(:ruby_method)).class }
try("an unbound")    { RubyVM::InstructionSequence.of(Object.instance_method(:ruby_method)).class }
try("a c method")    { RubyVM::InstructionSequence.of(method(:puts)).class }
try("a c row")       { RubyVM::InstructionSequence.of(Array.instance_method(:map)).class }
try("nil arg")       { RubyVM::InstructionSequence.of(nil).class }

# The location rows, which are what a source finder actually reads.
i = RubyVM::InstructionSequence.of(PR)
try("proc lineno")   { i.first_lineno }
try("proc path")     { File.basename(i.path) }
try("proc absolute") { i.absolute_path.end_with?("rubyvm_iseq_of_answers_for_a_ruby_callable.rb") }

m = RubyVM::InstructionSequence.of(method(:ruby_method))
try("m label")       { m.label }
try("m base_label")  { m.base_label }
try("m lineno")      { m.first_lineno }
try("m path")        { File.basename(m.path) }
__END__
a proc: RubyVM::InstructionSequence
a lambda: RubyVM::InstructionSequence
a ruby method: RubyVM::InstructionSequence
an unbound: RubyVM::InstructionSequence
a c method: NilClass
a c row: NilClass
nil arg: NilClass
proc lineno: 18
proc path: "rubyvm_iseq_of_answers_for_a_ruby_callable.rb"
proc absolute: true
m label: "ruby_method"
m base_label: "ruby_method"
m lineno: 17
m path: "rubyvm_iseq_of_answers_for_a_ruby_callable.rb"
