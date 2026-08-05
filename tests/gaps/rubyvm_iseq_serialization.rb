# The YARV-shaped corners of RubyVM. zeo compiles ahead of time and has no
# bytecode, so InstructionSequence#to_a/#to_binary/#disasm raise
# NotImplementedError naming that reality where CRuby answers real data;
# InstructionSequence.of answers nil for EVERY callable (CRuby builds a real
# iseq for a Ruby-defined proc); YJIT.enable truthfully answers false (there
# is no JIT to switch on -- CRuby answers true); and AST node ids are
# zeo-numbered (prism's parse-internal ids are not exposed through its Rust
# bindings). Each is a deliberate refusal or renumbering, not a missing
# surface -- see docs/COMPATIBILITY.md.
$stderr.reopen(IO::NULL)

iseq = RubyVM::InstructionSequence.compile("40 + 2")
begin
  p iseq.to_a[0]
rescue NotImplementedError => e
  p [:to_a, e.class]
end
begin
  p iseq.to_binary[0, 4]
rescue NotImplementedError => e
  p [:to_binary, e.class]
end
begin
  p iseq.disasm.lines.first.start_with?("== disasm")
rescue NotImplementedError => e
  p [:disasm, e.class]
end
p RubyVM::InstructionSequence.of(proc { 1 }).class
p RubyVM::YJIT.enable
p RubyVM::AbstractSyntaxTree.parse("x = 1 + 2\ny = x").node_id
