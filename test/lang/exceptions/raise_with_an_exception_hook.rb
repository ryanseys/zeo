# `raise obj, "msg"` where obj's class overrides #exception does not even
# BUILD: codegen hands the constructed object to
# `zeo_rt::coerce_raise_arg_with_message` as a raw `Arc<WithHook>` instead of
# wrapping it in `RubyValue::Object(...)`, and rustc rejects the generated
# program (E0308). Any program using this shape fails to compile, so this gap
# "diverges" by failing to produce a binary at all.
class WithHook
  def exception(msg = nil)
    RuntimeError.new("custom exception hook: #{msg}")
  end
end

begin
  raise WithHook.new, "hi"
rescue => e
  puts e.message
end

begin
  raise WithHook.new
rescue => e
  puts e.message
end
__END__
custom exception hook: hi
custom exception hook: 
