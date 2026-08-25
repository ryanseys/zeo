# Psych::SyntaxError carries the parse position as ACCESSORS (#line,
# #column, #file) and words its message `(<file>): <problem> while
# <context> at line L column C`. zeo's error has neither the accessors
# nor the shape. (Found by the 2026-08-24 probe sweep.)
require "yaml"
begin
  YAML.safe_load("ok: 1\nbad: [")
rescue Psych::SyntaxError => e
  p [e.line, e.column, e.file]
  puts e.message
end
