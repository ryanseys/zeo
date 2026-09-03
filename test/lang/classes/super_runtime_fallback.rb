# `super` with no definition above the defining class.
#
# Real Ruby resolves `super` at CALL time against the receiver's live ancestry
# (`vm_search_super_method`) and raises NoMethodError only when that walk comes
# up empty -- there is no definition-time check. The message comes from the one
# method-missing raiser, so it carries the same "for an instance of X" receiver
# description every other NoMethodError uses.

# 1. `super` into a method that exists nowhere above -> rescuable NoMethodError.
class Rec
  def as_json
    h = super
    h[:x] = 1
    h
  end
end

begin
  Rec.new.as_json
rescue NoMethodError => e
  puts e.message
end

# 2. `super` reaching a method that only an INCLUDED MODULE defines. A
#    compile-time scan of the superclass chain alone misses this; the runtime
#    walk covers the real ancestry, modules included.
module Greet
  def hi
    "[hi]"
  end
end

class C
  include Greet
  def hi
    super
  end
end

puts C.new.hi

# 3. Module-to-module `super` along the include chain.
module M1
  def tag
    "M1"
  end
end

module M2
  include M1
  def tag
    "M2(#{super})"
  end
end

class F
  include M2
  def tag
    "F[#{super}]"
  end
end

puts F.new.tag

# 4. The receiver description is shared with every other NoMethodError shape:
#    an instance, a class, a module, and nil each render differently.
begin
  Object.new.no_such_method
rescue NoMethodError => e
  puts e.message
end

begin
  nil.no_such_method
rescue NoMethodError => e
  puts e.message
end

module Helper; end

begin
  Helper.no_such_method
rescue NoMethodError => e
  puts e.message
end

begin
  String.no_such_method
rescue NoMethodError => e
  puts e.message
end
__END__
super: no superclass method 'as_json' for an instance of Rec
[hi]
F[M2(M1)]
undefined method 'no_such_method' for an instance of Object
undefined method 'no_such_method' for nil
undefined method 'no_such_method' for module Helper
undefined method 'no_such_method' for class String
