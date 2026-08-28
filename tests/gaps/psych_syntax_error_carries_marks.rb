require "yaml"
begin
  YAML.safe_load("ok: 1\nbad: [")
rescue Psych::SyntaxError => e
  p [e.line, e.column, e.file]
  puts e.message
end
