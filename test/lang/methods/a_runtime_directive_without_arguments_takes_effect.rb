# The argument-less runtime directives, forced through string `eval` so no
# compile-time resolution helps: a bare `private` switches the body default,
# a bare `module_function` switches the mode, and `include` on a compiled
# class splices the overlay ancestry. Each once read as a documented no-op
# in `runtime_meta/api.rs`; this golden keeps the docs honest.
puts "-- bare private via string eval --"
class Foo; end
eval(["Foo.class_eval do", "  private", "  def hidden = 'h'", "end"].join("\n"))
p Foo.private_method_defined?(:hidden)
begin
  Foo.new.hidden
rescue NoMethodError
  puts "NoMethodError"
end

puts "-- bare module_function via string eval --"
module M; end
eval(["M.module_eval do", "  module_function", "  def helper = 'help'", "end"].join("\n"))
p M.respond_to?(:helper)
p M.helper
p M.private_instance_methods.include?(:helper)

puts "-- runtime include on a compiled class via string eval --"
module Extra
  def extra = "extra"
end
class Compiled
  def base = "base"
end
eval(["Compiled", ".include(Extra)"].join(""))
p Compiled.include?(Extra)
p Compiled.ancestors.include?(Extra)
begin
  p Compiled.new.extra
rescue NoMethodError
  puts "NoMethodError"
end
__END__
-- bare private via string eval --
true
NoMethodError
-- bare module_function via string eval --
true
"help"
true
-- runtime include on a compiled class via string eval --
true
true
"extra"
