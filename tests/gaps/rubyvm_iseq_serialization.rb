# The YARV-shaped corners of RubyVM. zeo compiles ahead of time and has no
# bytecode, so InstructionSequence#to_a/#to_binary/#disasm raise
# NotImplementedError naming that reality where CRuby answers real data;
# InstructionSequence.of answers nil for EVERY callable (CRuby builds a real
# iseq for a Ruby-defined proc); YJIT.enable truthfully answers false (there
# is no JIT to switch on -- CRuby answers true); and AST node ids are
# zeo-numbered (prism's parse-internal ids are not exposed through its Rust
# bindings). Each is a deliberate refusal or renumbering, not a missing
# surface -- see docs/COMPATIBILITY.md.
#
# Read the four rows apart -- they are not one gap:
#
#   to_a / to_binary / disasm   PERMANENT. There is no bytecode to serialize.
#                               A faithful answer would mean emitting YARV
#                               zeo never runs.
#   InstructionSequence.of      Answers nil for every callable, where CRuby
#                               builds a real iseq for a Ruby-defined proc.
#                               Permanent for the same reason; a caller using
#                               it to ask "was this defined in Ruby?" gets the
#                               wrong answer rather than a refusal, which is
#                               the one row here that is quietly wrong instead
#                               of loudly absent.
#   YJIT.enable                 Truthful, not a gap in spirit: there is no JIT
#                               to switch on, so `false` is the honest answer
#                               where CRuby's `true` reports a real state
#                               change. Listed so a caller that branches on it
#                               is not surprised.
#   AST node_id                 FIXABLE, unlike the rest -- but NOT for the
#                               reason recorded here until 2026-08-21, which
#                               was wrong twice over. (1) prism's ids ARE
#                               reachable: the safe crate keeps `pointer`
#                               private on each node STRUCT, but `Node` is an
#                               enum and a variant's fields are public, so
#                               `Node::CallNode { pointer, .. }` reads
#                               `pm_node_t.node_id`. (2) reaching them would
#                               not help anyway: CRuby's AST ids are a THIRD
#                               numbering, neither prism's nor zeo's
#                               (`nil.nope` is prism id 3 for the call and
#                               CRuby AST id 1). Matching this row means
#                               reproducing CRuby's own allocation order,
#                               which is the same work as completing the
#                               translator -- see
#                               `the_ast_translator_answers_unknown_for_some_shapes.rb`.
#                               prism's real ids are wanted elsewhere, by
#                               `ast_node_id_for_backtrace_location.rb`.
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
