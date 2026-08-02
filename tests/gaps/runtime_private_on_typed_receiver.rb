# Visibility is a RUNTIME property in ruby: `private :name` re-marks a method
# that already exists, and every later call sees the new mark. zeo answers a
# call whose receiver class is known at compile time (Path 1) with a direct
# call to the generated function, and decides visibility once, when it emits
# that call (`codegen/call/visibility.rs`, `enforce_visibility`). A `private`
# sent afterwards cannot reach a decision already compiled in.
#
# The dynamic half is correct: a Path 2 site asks the runtime on every call it
# does not serve from its inline cache, so the same program written against an
# untyped receiver raises (see tests/explicit_receiver_visibility.rb).
#
# Fix shape: a class the program marks at run time is already tracked -- the
# runtime overlay's `is_live()` gate, and the compiler's
# `may_be_undefined_at_runtime` for `undef`. Give visibility the same treatment:
# when the compiler sees ANY `private`/`public`/`protected` sent to a class as a
# message rather than written in its body, stop folding Path 1 visibility for
# that class's methods and let the site ask the runtime.

class Late
  def open_now = :open
end

holder = Late.new
p holder.open_now

Late.send(:private, :open_now)

begin
  p holder.open_now
rescue NoMethodError => e
  puts e.message
end
