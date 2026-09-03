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
__END__
"1" => (SCOPE [1:0-1:1] [] nil (INTEGER [1:0-1:1] 1))
"1 + 2" => (SCOPE [1:0-1:5] [] nil (OPCALL [1:0-1:5] (INTEGER [1:0-1:1] 1) :+ (LIST [1:4-1:5] (INTEGER [1:4-1:5] 2) nil)))
"x = 5" => (SCOPE [1:0-1:5] [:x] nil (LASGN [1:0-1:5] :x (INTEGER [1:4-1:5] 5)))
"x = 5; x" => (SCOPE [1:0-1:8] [:x] nil (BLOCK [1:0-1:8] (LASGN [1:0-1:5] :x (INTEGER [1:4-1:5] 5)) (LVAR [1:7-1:8] :x)))
"puts \"hi\"" => (SCOPE [1:0-1:9] [] nil (FCALL [1:0-1:9] :puts (LIST [1:5-1:9] (STR [1:5-1:9] "hi") nil)))
"foo" => (SCOPE [1:0-1:3] [] nil (VCALL [1:0-1:3] :foo))
"a.b" => (SCOPE [1:0-1:3] [] nil (CALL [1:0-1:3] (VCALL [1:0-1:1] :a) :b nil))
"def m(a, b = 1) a end" => (SCOPE [1:0-1:21] [] nil (DEFN [1:0-1:21] :m (SCOPE [1:0-1:21] [:a, :b] (ARGS [1:6-1:14] 1 nil (OPT_ARG [1:9-1:14] (LASGN [1:9-1:14] :b (INTEGER [1:13-1:14] 1)) nil) nil 0 nil nil nil nil nil) (LVAR [1:16-1:17] :a))))
"if x then 1 else 2 end" => (SCOPE [1:0-1:22] [] nil (IF [1:0-1:22] (VCALL [1:3-1:4] :x) (INTEGER [1:10-1:11] 1) (INTEGER [1:17-1:18] 2)))
"[1, :two, \"three\"]" => (SCOPE [1:0-1:18] [] nil (LIST [1:0-1:18] (INTEGER [1:1-1:2] 1) (SYM [1:4-1:8] :two) (STR [1:10-1:17] "three") nil))
"{ a: 1 }" => (SCOPE [1:0-1:8] [] nil (HASH [1:0-1:8] (LIST [1:2-1:6] (SYM [1:2-1:4] :a) (INTEGER [1:5-1:6] 1) nil)))
"C = 1" => (SCOPE [1:0-1:5] [] nil (CDECL [1:0-1:5] :C (INTEGER [1:4-1:5] 1)))
"while true do break end" => (SCOPE [1:0-1:23] [] nil (WHILE [1:0-1:23] (TRUE [1:6-1:10] ) (BREAK [1:14-1:19] nil) true))
"a { |x| x }" => (SCOPE [1:0-1:11] [] nil (ITER [1:0-1:11] (FCALL [1:0-1:1] :a nil) (SCOPE [1:2-1:11] [:x] (ARGS [1:5-1:6] 1 nil nil nil 0 nil nil nil nil nil) (DVAR [1:8-1:9] :x))))
"1..2" => (SCOPE [1:0-1:4] [] nil (DOT2 [1:0-1:4] (INTEGER [1:0-1:1] 1) (INTEGER [1:3-1:4] 2)))
"class Foo; end" => (SCOPE [1:0-1:14] [] nil (CLASS [1:0-1:14] (COLON2 [1:6-1:9] nil :Foo) nil (SCOPE [1:0-1:14] [] nil (BEGIN [1:9-1:9] nil))))
"return 1 if false" => (SCOPE [1:0-1:17] [] nil (IF [1:0-1:17] (FALSE [1:12-1:17] ) (RETURN [1:0-1:8] (INTEGER [1:7-1:8] 1)) nil))
"@iv = 1" => (SCOPE [1:0-1:7] [] nil (IASGN [1:0-1:7] :@iv (INTEGER [1:6-1:7] 1)))
"nil" => (SCOPE [1:0-1:3] [] nil (NIL [1:0-1:3] ))
"true or false" => (SCOPE [1:0-1:13] [] nil (OR [1:0-1:13] (TRUE [1:0-1:4] ) (FALSE [1:8-1:13] )))
RubyVM::AbstractSyntaxTree::Node
:SCOPE
[1, 0, 2, 5]
true
:BLOCK
["x = 1 + 2\n", "y = x"]
"x = 1 + 2\ny = x"
"x = 1 + 2\ny = x"
"#<RubyVM::AbstractSyntaxTree::Node:SCOPE@1:0-2:5>"
[RubyVM::AbstractSyntaxTree::Location]
"#<RubyVM::AbstractSyntaxTree::Location:@1:0-2:5>"
[1, 5]
[nil, nil, nil, nil]
SyntaxError
:SCOPE
[RuntimeError, "cannot get AST for ISEQ compiled by prism"]
nil
[:SCOPE, :LASGN]
[:first_column, :first_lineno, :inspect, :last_column, :last_lineno]
[TypeError, "allocator undefined for RubyVM::AbstractSyntaxTree::Location"]
[:keep_script_lines, :keep_script_lines=, :stat]
[:AbstractSyntaxTree, :DEFAULT_PARAMS, :INSTRUCTION_NAMES, :InstructionSequence, :OPTS, :YJIT]
false
true
[:constant_cache_invalidations, :constant_cache_misses, :global_cvar_state, :next_shape_id, :shape_cache_size]
true
true
[ArgumentError, "unknown key: bogus"]
[Array, false]
[Array, true]
[[:fiber_machine_stack_size, :fiber_vm_stack_size, :thread_machine_stack_size, :thread_vm_stack_size], true]
[false, false, false, false]
[nil, nil, nil]
[ArgumentError, "--yjit-trace-exits must be enabled to use dump_exit_locations."]
[nil, nil]
RubyVM::InstructionSequence
42
["<compiled>", "<compiled>", "<compiled>", "<compiled>", 1]
"<RubyVM::InstructionSequence:<compiled>@<compiled>:1>"
[nil, [[1, :line]]]
Hash
[RuntimeError, "broken binary format"]
SyntaxError
"myfile.rb"
7
6
["<main>", "/tmp/zeo_ast_fixture.rb"]
42
