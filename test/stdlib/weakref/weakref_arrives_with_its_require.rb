# `WeakRef` before `require "weakref"`.
p defined?(WeakRef)
p Object.constants.include?(:WeakRef)
begin
  WeakRef
rescue NameError => e
  puts e.message
end
require "weakref"
p defined?(WeakRef), defined?(WeakRef::RefError)
o = Object.new
r = WeakRef.new(o)
p r.weakref_alive?, r.__getobj__.equal?(o)
__END__
nil
false
uninitialized constant WeakRef
"constant"
"constant"
true
true
