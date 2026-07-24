# A method compiled but never called whose body splits an unresolvable chain
# (`Undefined.application.class.to_s.split("::")`) must still produce
# compilable C: the chain resolves to a poly nil at codegen even though it is
# typed String, so the split receiver is coerced (sp_poly_to_s) rather than
# emitted as a raw sp_box_nil() where a const char* is expected.
class T
  def initialize(s)
    @s = s
  end
  def name
    @s[:name] || fallback
  end
  def fallback
    Undefined.application.class.to_s.split("::").first
  end
end

puts T.new({ name: "ok" }).name
