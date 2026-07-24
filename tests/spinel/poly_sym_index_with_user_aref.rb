# Defining an instance `[]` must not drop the builtin Hash#[] case from the
# polymorphic `[]` dispatch: a symbol-keyed Hash indexed through a poly (block
# param) receiver was lowered to a cls_id switch built only from user `[]`
# methods, so the Hash receiver fell through to nil. The dispatch must also
# carry the SymPolyHash arm.
class Box
  def [](key)
    nil
  end
end

pins = [{ name: "app", path: "/a.js" }, { name: "lib", path: "/b.js" }]
pins.each { |p| puts p[:name] + " -> " + p[:path] }
puts pins.map { |p| p[:name] }.join(",")

# the user-defined [] still dispatches for a real Box receiver
b = Box.new
p b[:anything].nil?
