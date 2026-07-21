# Test Data.define
#
# `Foo = Data.define(:a, :b)` is structurally identical to
# `Foo = Struct.new(:a, :b)` from Spinel's analyzer perspective:
# the positional symbol args list the field names of a synthetic
# class. The Data class itself is keyword-init at runtime
# (`Foo.new(a:, b:)`), which Spinel's existing kwarg-constructor
# path matches by name.
#
# A prior bug skipped the `Data.define` shape in collect_scoped_constant,
# so `obj_<Foo>`-typed locals downstream tripped clang
# `unknown type name 'sp_<Foo>'` because no class was ever registered.

Point = Data.define(:x, :y)

p1 = Point.new(x: 3, y: 4)
puts p1.x       # 3
puts p1.y       # 4

# A second Data class in the same scope — confirms multiple
# synthetic Data classes can coexist.
Color = Data.define(:r, :g, :b)

c = Color.new(r: 255, g: 128, b: 0)
puts c.r        # 255
puts c.g        # 128
puts c.b        # 0
