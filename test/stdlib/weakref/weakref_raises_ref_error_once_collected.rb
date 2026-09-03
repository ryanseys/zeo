# `nil`, not `false`: weakref.rb spells it `@@__map.key?(self) or
# defined?(@delegate_sd_obj)`, and `defined?` answers nil.

require "weakref"
def make = WeakRef.new(Object.new)
w = make
GC.start
p w.weakref_alive?
begin
  w.some_method
rescue WeakRef::RefError => e
  puts "RefError: #{e.message}"
end
begin
  w.__getobj__
rescue WeakRef::RefError => e
  puts "getobj: #{e.class}"
end
# WeakRef::RefError is a StandardError, so a bare rescue catches it
p WeakRef::RefError.superclass
p WeakRef::RefError.ancestors.include?(StandardError)
__END__
nil
RefError: Invalid Reference - probably recycled
getobj: WeakRef::RefError
StandardError
true
