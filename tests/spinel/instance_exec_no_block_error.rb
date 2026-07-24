# Blockless instance_exec is LocalJumpError; blockless instance_eval (no
# source string either) is the arity ArgumentError. Both oracle shapes.
o = Object.new
begin
  o.instance_exec
rescue LocalJumpError => e
  puts "exec: #{e.class}: #{e.message}"
end
begin
  o.instance_eval
rescue ArgumentError => e
  puts "eval: #{e.class}: #{e.message}"
end
