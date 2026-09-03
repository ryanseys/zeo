# `to_binary` is CRuby's version-stamped internal IBF format, keyed to a VM
# zeo does not have. There is no honest answer, so it refuses.
#
# `to_a` and `disasm` are the real question, and the answer is still no. Both
# would mean EMITTING YARV that zeo never runs -- a second backend whose only
# consumer is reflection, kept correct against a bytecode format that changes
# every ruby release, with no program whose behaviour depends on it. The
# `NotImplementedError` names that reality; a plausible-looking fake would be
# worse, because a caller reading it has no way to tell.
#
# `YJIT.enable` is truthful rather than equal. There is no JIT to switch on,
# so `false` is the honest answer where CRuby's `true` reports a real state
# change. Listed so a caller that branches on it is not surprised.
#
# The AST `node_id` row is context and it MATCHES: zeo's numbering agrees with
# CRuby's here. (The note this file carried until 2026-08-26 said prism's ids
# were unreachable through the Rust bindings and that they were the answer.
# Both halves were wrong -- `Node` is an enum, so a variant's `pointer` field
# reads `pm_node_t.node_id`, and CRuby's AST ids are a THIRD numbering,
# neither prism's nor zeo's.)
#
# The one row that is genuinely still open -- `#label` on a Proc-derived
# handle -- lives in `tests/gaps/rubyvm_iseq_label_on_a_proc_handle.rb`. It
# was split out because a gap file mixing a permanent refusal with a fixable
# row can never promote, which made the fixable row invisible work.
#
# --- ruby 4.0.6 answers ---
# to_a	YARVInstructionSequence/SimpleDataFormat
# to_binary	YARB
# disasm	true
# true
# 9

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
__END__
to_a	NotImplementedError
to_binary	NotImplementedError
disasm	NotImplementedError
false
9
