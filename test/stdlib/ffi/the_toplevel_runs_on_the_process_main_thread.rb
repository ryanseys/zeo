#@ only: macos
# The top level runs on the PROCESS main thread (`pthread_main_np` == 1),
# the thread Apple's frameworks insist on: AppKit raises
# `NSInternalInconsistencyException: NSWindow should only be instantiated on
# the main thread!` from any other, WebKit, Metal and the Cocoa run loop share
# the rule, and no dispatch trick satisfies the check. zeo once ran the top
# level on a spawned 64 MiB thread; the darwin link line now sizes the main
# thread's stack instead (`-stack_size`, see `link_binary`). The portable
# half -- `Thread.current` at the top level is `Thread.main` -- is
# tests/the_toplevel_thread_is_the_main_thread.rb.

require "ffi"
module AK
  extend FFI::Library
  ffi_lib "objc"
  ffi_lib "/System/Library/Frameworks/AppKit.framework/AppKit"
  class CGPoint < FFI::Struct
    layout :x, :double, :y, :double
  end
  class CGSize < FFI::Struct
    layout :width, :double, :height, :double
  end
  class NSRect < FFI::Struct
    layout :origin, CGPoint, :size, CGSize
  end
  attach_function :objc_getClass, [:string], :pointer
  attach_function :sel_registerName, [:string], :pointer
  attach_function :send_id, :objc_msgSend, [:pointer, :pointer], :pointer
  attach_function :send_init_rect, :objc_msgSend, [:pointer, :pointer, NSRect.by_value, :ulong, :ulong, :bool], :pointer
  attach_function :pthread_main_np, [], :int
end
def sel(s) = AK.sel_registerName(s)
puts "main thread: #{AK.pthread_main_np}"
puts "Thread.main: #{Thread.current.equal?(Thread.main)}"
r = AK::NSRect.new
r[:origin][:x] = 100
r[:origin][:y] = 200
r[:size][:width] = 640
r[:size][:height] = 480
win = AK.send_init_rect(AK.send_id(AK.objc_getClass("NSWindow"), sel("alloc")), sel("initWithContentRect:styleMask:backing:defer:"), r, 15, 2, false)
puts "window: #{win.null? ? "NULL" : "ok"}"
puts "worker: #{Thread.new { AK.pthread_main_np }.value}"
__END__
main thread: 1
Thread.main: true
window: ok
worker: 0
