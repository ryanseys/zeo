# The YARV-shaped corners of RubyVM. zeo compiles ahead of time and has no
# bytecode, so these rows are refusals with a reason rather than unbuilt
# work -- the `.divergence` sidecar carries each one.
#
# The one row that IS still fixable was split out into
# `tests/gaps/rubyvm_iseq_label_on_a_proc_handle.rb`, so this file can be a
# decided divergence and that one can flip on its own.
$stderr.reopen(IO::NULL)

iseq = RubyVM::InstructionSequence.compile("40 + 2")
{
  to_a: -> { iseq.to_a[0] },
  to_binary: -> { iseq.to_binary[0, 4] },
  disasm: -> { iseq.disasm.lines.first.start_with?("== disasm") },
}.each do |name, fn|
  r = begin
    fn.call
  rescue Exception => e
    e.class.to_s
  end
  puts "#{name}\t#{r}"
end

# Truthful rather than equal: there is no JIT to switch on, so `false` is
# the honest answer where CRuby's `true` reports a real state change.
p RubyVM::YJIT.enable

# Context, and exact: the AST node id matches CRuby's own numbering here.
p RubyVM::AbstractSyntaxTree.parse("x = 1 + 2\ny = x").node_id
