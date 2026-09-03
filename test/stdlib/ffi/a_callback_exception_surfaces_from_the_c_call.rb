#@ only: macos
# An IMP written in Ruby and installed with `class_addMethod`: Objective-C
# reaches it through `objc_msgSend`, an attached function that takes no
# callback argument of its own. The gem still raises the exception the IMP
# raised from that call -- so every forward call asks for a stashed callback
# error on return, not only one that passed a callback. A `:void` IMP is the
# ordinary shape of a notification handler, and builds.

require "ffi"
module OC
  extend FFI::Library
  ffi_lib "objc"
  ffi_lib "/System/Library/Frameworks/Foundation.framework/Foundation"
  attach_function :objc_getClass, [:string], :pointer
  attach_function :sel_registerName, [:string], :pointer
  attach_function :objc_allocateClassPair, [:pointer, :string, :size_t], :pointer
  attach_function :objc_registerClassPair, [:pointer], :void
  attach_function :class_addMethod, [:pointer, :pointer, :pointer, :string], :bool
  attach_function :send_id, :objc_msgSend, [:pointer, :pointer], :pointer
  attach_function :send_long_ret, :objc_msgSend, [:pointer, :pointer], :long
  attach_function :send_void_obj, :objc_msgSend, [:pointer, :pointer, :pointer], :void
end
def sel(s) = OC.sel_registerName(s)
calls = []
klass = OC.objc_allocateClassPair(OC.objc_getClass("NSObject"), "ZeoProbe", 0)
imps = []
imps << (f = FFI::Function.new(:void, [:pointer, :pointer, :pointer], proc { |_s, _c, _a| calls << :void; nil }))
OC.class_addMethod(klass, sel("note:"), f, "v@:@")
imps << (f = FFI::Function.new(:long, [:pointer, :pointer], proc { |_s, _c| raise "boom inside imp" }))
OC.class_addMethod(klass, sel("explode"), f, "q@:")
imps << (f = FFI::Function.new(:void, [:pointer, :pointer, :pointer]) { |_s, _c, _a| calls << :block })
OC.class_addMethod(klass, sel("noteBlock:"), f, "v@:@")
OC.objc_registerClassPair(klass)
obj = OC.send_id(OC.send_id(klass, sel("alloc")), sel("init"))
OC.send_void_obj(obj, sel("note:"), obj)
OC.send_void_obj(obj, sel("noteBlock:"), obj)
p calls
begin
  OC.send_long_ret(obj, sel("explode"))
rescue => e
  p [:rescued, e.class, e.message]
end
p OC.send_long_ret(obj, sel("hash")).class
puts "done"
__END__
[:void, :block]
[:rescued, RuntimeError, "boom inside imp"]
Integer
done
