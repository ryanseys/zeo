# RubyVM::AbstractSyntaxTree over prism with parse.y taxonomy: the tree
# shapes below are the oracle's own dumps (type, location, children) for the
# tier of constructs zeo maps faithfully. node_id values are zeo-numbered
# (prism's ids are parse-internal), so only their presence is pinned.
$stderr.reopen(IO::NULL)

def dump(n)
  return "nil" if n.nil?
  unless n.is_a?(RubyVM::AbstractSyntaxTree::Node)
    return n.inspect
  end
  kids = n.children.map { |c| dump(c) }
  "(#{n.type} [#{n.first_lineno}:#{n.first_column}-#{n.last_lineno}:#{n.last_column}] #{kids.join(' ')})"
end

[
  "1",
  "1 + 2",
  "x = 5",
  "x = 5; x",
  "puts \"hi\"",
  "foo",
  "a.b",
  "def m(a, b = 1) a end",
  "if x then 1 else 2 end",
  "[1, :two, \"three\"]",
  "{ a: 1 }",
  "C = 1",
  "while true do break end",
  "a { |x| x }",
  "1..2",
  "class Foo; end",
  "return 1 if false",
  "@iv = 1",
  "nil",
  "true or false",
].each do |src|
  ast = RubyVM::AbstractSyntaxTree.parse(src)
  puts "#{src.inspect} => #{dump(ast)}"
end

# ---- the Node surface.
ast = RubyVM::AbstractSyntaxTree.parse("x = 1 + 2\ny = x", keep_script_lines: true)
p ast.class
p ast.type
p [ast.first_lineno, ast.first_column, ast.last_lineno, ast.last_column]
p ast.node_id.is_a?(Integer)
p ast.children.last.type
p ast.script_lines
p ast.source
p ast.children.last.source
p ast.inspect
p ast.locations.map(&:class)
p ast.locations.first.inspect
p [ast.locations.first.first_lineno, ast.locations.first.last_column]
plain = RubyVM::AbstractSyntaxTree.parse("x=1")
p [plain.script_lines, plain.source, plain.tokens, plain.all_tokens]

# ---- errors and options.
begin
  RubyVM::AbstractSyntaxTree.parse("def broken(")
rescue SyntaxError => e
  p e.class
end
p RubyVM::AbstractSyntaxTree.parse("def broken(", error_tolerant: true).type
begin
  RubyVM::AbstractSyntaxTree.of(proc { 1 })
rescue RuntimeError => e
  p [e.class, e.message]
end
p RubyVM::AbstractSyntaxTree.of(method(:puts))
File.write("/tmp/zeo_ast_fixture.rb", "z = 42\n")
pf = RubyVM::AbstractSyntaxTree.parse_file("/tmp/zeo_ast_fixture.rb")
p [pf.type, pf.children.last.type]

# ---- Location.
p RubyVM::AbstractSyntaxTree::Location.instance_methods(false).sort
begin
  RubyVM::AbstractSyntaxTree::Location.new
rescue TypeError => e
  p [e.class, e.message]
end

# ---- RubyVM itself.
p RubyVM.singleton_methods(false).sort
p RubyVM.constants.sort
p RubyVM.keep_script_lines
RubyVM.keep_script_lines = true
p RubyVM.keep_script_lines
RubyVM.keep_script_lines = false
p RubyVM.stat.keys.sort
p RubyVM.stat.values.all? { |v| v.is_a?(Integer) }
p RubyVM.stat(:constant_cache_invalidations).is_a?(Integer)
begin
  RubyVM.stat(:bogus)
rescue ArgumentError => e
  p [e.class, e.message]
end
p [RubyVM::OPTS.class, RubyVM::OPTS.frozen?]
p [RubyVM::INSTRUCTION_NAMES.class, RubyVM::INSTRUCTION_NAMES.frozen?]
p [RubyVM::DEFAULT_PARAMS.keys.sort, RubyVM::DEFAULT_PARAMS.frozen?]

# ---- YJIT, never enabled.
y = RubyVM::YJIT
p [y.enabled?, y.stats_enabled?, y.log_enabled?, y.trace_exit_locations_enabled?]
p [y.runtime_stats, y.exit_locations, y.log]
begin
  y.dump_exit_locations("/tmp/x")
rescue ArgumentError => e
  p [e.class, e.message]
end
p [y.code_gc, y.reset_stats!]

# ---- InstructionSequence.
iseq = RubyVM::InstructionSequence.compile("40 + 2")
p iseq.class
p iseq.eval
p [iseq.label, iseq.base_label, iseq.path, iseq.absolute_path, iseq.first_lineno]
p iseq.inspect
p [iseq.script_lines, iseq.trace_points]
p RubyVM::InstructionSequence.compile_option.class
begin
  RubyVM::InstructionSequence.load_from_binary("x")
rescue RuntimeError => e
  p [e.class, e.message]
end
begin
  RubyVM::InstructionSequence.compile("def broken(")
rescue SyntaxError => e
  p e.class
end
p RubyVM::InstructionSequence.compile("1", "myfile.rb").path
p RubyVM::InstructionSequence.compile("1", "myfile.rb", "/abs/myfile.rb", 7).first_lineno
p RubyVM::InstructionSequence.new("2 * 3").eval
f = RubyVM::InstructionSequence.compile_file("/tmp/zeo_ast_fixture.rb")
p [f.label, f.path]
p f.eval
