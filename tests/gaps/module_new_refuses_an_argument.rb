# `Module.new` takes NO positional argument -- only an optional block, which
# it module_evals. zeo accepts one and answers a module.
#
# Carved out of tests/probe_arguments.rb ("Module.new(arg)"), which cannot
# gate the row until this passes.
begin
  m = Module.new(1)
  puts "ok #{m.class}"
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
