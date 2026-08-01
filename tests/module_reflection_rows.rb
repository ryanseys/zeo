# `Module`'s reflection surface: the constant hooks, the class-variable
# removal, the undef listing, and the lexical-nesting query.

module Outer
  INNER_CONST = 1
  module Inner
    puts "nesting inner: #{Module.nesting.inspect}"
  end
  puts "nesting outer: #{Module.nesting.inspect}"
end
puts "nesting top: #{Module.nesting.inspect}"

class Named
  puts "nesting class: #{Module.nesting.inspect}"
end

# `const_source_location`: an Array for a constant that exists (CRuby fills in
# the file and line; zeo answers the empty Array it gives every C-defined
# constant), nil for one that does not, NameError for a bad name.
puts "source loc known: #{Outer.const_source_location(:INNER_CONST).class}"
puts "source loc missing: #{Object.const_source_location(:Nope).inspect}"
begin
  Object.const_source_location("nope")
rescue NameError => e
  puts "source loc bad: #{e.message}"
end

# The default `const_missing` raises the NameError a plain miss would.
begin
  Outer.const_missing(:Zed)
rescue NameError => e
  puts "const_missing: #{e.message}"
end

# `autoload` resolves at compile time, so both rows answer nil -- what CRuby
# answers once the feature has loaded.
puts "autoload: #{Outer.autoload(:Later, 'later').inspect}"
puts "autoload?: #{Outer.autoload?(:Later).inspect}"

# `undefined_instance_methods` -- Complex is the one core class that undefs.
puts "complex undefs: #{Complex.undefined_instance_methods.sort.inspect}"
puts "object undefs: #{Object.undefined_instance_methods.inspect}"

class Undefer
  def gone; end
  undef gone
end
puts "user undefs: #{Undefer.undefined_instance_methods.inspect}"
puts "user instance methods: #{Undefer.instance_methods(false).inspect}"

# `public_instance_method` refuses a private method by name.
class Secretive
  def open_door; end
  private def shut_door; end
end
puts "public im: #{Secretive.public_instance_method(:open_door).class}"
begin
  Secretive.public_instance_method(:shut_door)
rescue NameError => e
  puts "public im private: #{e.message}"
end
begin
  Secretive.public_instance_method(:absent)
rescue NameError => e
  puts "public im absent: #{e.message}"
end

# `remove_class_variable` answers the value it dropped.
class Holder
  @@count = 42
end
puts "removed: #{Holder.remove_class_variable(:@@count)}"
puts "after remove: #{Holder.class_variables.inspect}"
begin
  Holder.remove_class_variable(:@@count)
rescue NameError => e
  puts "remove twice: #{e.message}"
end

# `set_temporary_name` only takes on a class with no constant path.
anon = Class.new
puts "anon name: #{anon.name.inspect}"
anon.set_temporary_name("scratch")
puts "temp name: #{anon.name.inspect}"
anon.set_temporary_name(nil)
puts "cleared name: #{anon.name.inspect}"
begin
  anon.set_temporary_name("")
rescue ArgumentError => e
  puts "temp empty: #{e.message}"
end
begin
  anon.set_temporary_name("Scratch")
rescue ArgumentError => e
  puts "temp const path: #{e.message}"
end
begin
  Named.set_temporary_name("nope")
rescue RuntimeError => e
  puts "temp permanent: #{e.message}"
end

puts "refinements: #{Outer.refinements.inspect}"
puts "used_modules: #{Module.used_modules.inspect}"
puts "used_refinements: #{Module.used_refinements.inspect}"
