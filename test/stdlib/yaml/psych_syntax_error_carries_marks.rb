require "yaml"
begin
  YAML.safe_load("ok: 1\nbad: [")
rescue Psych::SyntaxError => e
  p [e.line, e.column, e.file]
  puts e.message
end

begin
  YAML.parse("x: [", filename: "f.yml")
rescue Psych::SyntaxError => e
  p [e.file, e.line, e.column]
end

begin
  YAML.load("- a\n- \"b\n")
rescue Psych::SyntaxError => e
  p [e.line, e.column, e.problem]
end

begin
  YAML.load("a: *nope\n")
rescue => e
  p [e.class.to_s, e.message]
end

p Psych::SyntaxError.instance_methods(false).sort
__END__
[3, 1, nil]
(<unknown>): did not find expected node content while parsing a flow node at line 3 column 1
["f.yml", 2, 1]
[2, 3, "found unexpected end of stream"]
["Psych::AliasesNotEnabled", "Alias parsing was not enabled. To enable it, pass `aliases: true` to `Psych::load` or `Psych::safe_load`."]
[:column, :context, :file, :line, :offset, :problem]
