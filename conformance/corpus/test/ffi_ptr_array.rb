# #492. An array literal whose elements are FFI `:ptr` returns
# (e.g. `[LibC.malloc(...)]`) used to infer as `int_array`, since
# `infer_array_elem_type_from_ids` had no `et == "ptr"` arm and
# fell through to the int_array default. Codegen then emitted
# `sp_IntArray *` storage and `sp_IntArray_push(arr, void *)`,
# which fails -Wint-conversion on every push site — the user hit
# this in a transformer-LM workload with per-head FFI ptr slots.
#
# Fix: analyze + codegen recognise `[ptr, ptr, ...]` and lower it
# to `ptr_ptr_array` (sp_PtrArray with the noscan constructor —
# external pointers don't carry sp_gc_hdr, so the default
# sp_gc_mark element scan would crash at collection time).
#
# Test asserts the program runs to completion + prints the length.
# The crucial regression is at codegen — that the generated C now
# compiles under -Werror=int-conversion. The harness drops cc
# stderr, so failure surfaces as a missing binary / wrong output
# rather than a noisy warning.

module LibC
  ffi_func :malloc, [:size_t], :ptr
  ffi_func :free,   [:ptr],    :void
end

class Bag
  attr_accessor :slots
  def initialize
    @slots = [LibC.malloc(8)]
  end
end

b = Bag.new
b.slots.push(LibC.malloc(8))
puts b.slots.length
b.slots.each { |p| LibC.free(p) }
