# Classes and modules answer reflection queries about their constants,
# methods, and class variables.
module Config
  VERSION = "1.0"
  @@count = 0
end

# const_get / const_defined? resolve a literal name against the class's
# constant registry (a String or Symbol name both work).
puts Config.const_get(:VERSION)         # 1.0
p Config.const_defined?(:VERSION)       # true
p Config.const_defined?("MISSING")      # false

# A missing constant raises NameError; a malformed name is a different error.
begin
  Config.const_get(:Nope)
rescue NameError => e
  puts e.message                        # uninitialized constant Config::Nope
end
begin
  Config.const_defined?("lower")
rescue NameError => e
  puts e.message                        # wrong constant name lower
end

# method_defined? walks the ancestry, so inherited methods count too.
class Animal
  def breathe; end
end
class Dog < Animal
  def bark; end
end
p Dog.method_defined?(:bark)            # true (own)
p Dog.method_defined?(:breathe)         # true (inherited)
p Dog.method_defined?(:object_id)       # true (from Object)
p Dog.method_defined?(:nope)            # false

# class_variable_get/set/defined? reflect over @@ storage.
p Config.class_variable_defined?(:@@count)  # true
Config.class_variable_set(:@@count, 7)
p Config.class_variable_get(:@@count)       # 7
