# Three namespaces ruby carries that zeo lacked, none of which has a method
# of its own -- they are names, and a name is the whole contract.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e
  puts "#{label}: #{e.class}: #{e.message}"
end

# `UnicodeNormalize` is an EMPTY module: the namespace `String`'s
# `#unicode_normalize` family is documented under.
show("UnicodeNormalize class") { UnicodeNormalize.class }
show("UnicodeNormalize methods") { UnicodeNormalize.instance_methods(false) }
show("UnicodeNormalize singletons") { UnicodeNormalize.singleton_methods(false) }
show("UnicodeNormalize constants") { UnicodeNormalize.constants(false) }
show("UnicodeNormalize name") { UnicodeNormalize.name }

# `Set::CoreSet` is a Set subclass with no methods of its own, and NOT the
# same object as `Set`.
show("CoreSet name") { Set::CoreSet.name }
show("CoreSet superclass") { Set::CoreSet.superclass }
show("CoreSet ancestors") { Set::CoreSet.ancestors.take(3) }
show("CoreSet own methods") { Set::CoreSet.instance_methods(false) }
show("CoreSet is not Set") { Set::CoreSet.equal?(Set) }

# ruby defines `BasicObject` as a constant on ITSELF, which is why its own
# constant list is not empty where every other class's is.
show("BasicObject constants") { BasicObject.constants(false) }
show("BasicObject::BasicObject") { BasicObject::BasicObject }
show("Object constants(false) has no self") { Object.constants(false).include?(:Object) }
__END__
UnicodeNormalize class: Module
UnicodeNormalize methods: []
UnicodeNormalize singletons: []
UnicodeNormalize constants: []
UnicodeNormalize name: "UnicodeNormalize"
CoreSet name: "Set::CoreSet"
CoreSet superclass: Set
CoreSet ancestors: [Set::CoreSet, Set, Enumerable]
CoreSet own methods: []
CoreSet is not Set: false
BasicObject constants: [:BasicObject]
BasicObject::BasicObject: BasicObject
Object constants(false) has no self: true
