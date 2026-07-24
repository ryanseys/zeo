# #509. `Class#name` (and `self.name`) inside `def self.X` body
# previously fell through to the unresolved-call warning and
# emitted 0. Used by ActiveRecord's `"#{name}.table_name must
# be overridden"` raise pattern (4 sites in roundhouse).
#
# Fix: bare `name` in cmeth context routes to
# sp_class_to_s({@current_class_idx}) via compile_no_recv_call_expr;
# `self.name` is special-cased in compile_object_method_expr
# (since SelfNode there types as obj_<C> and the obj-method
# dispatch has no `name` arm). Analyze types both forms as
# `string`.

class Foo
  def self.who
    "I am #{name}"
  end
  def self.who_self
    "I am #{self.name}"
  end
end

class Bar
  def self.greet
    name + " says hi"
  end
end

puts Foo.who
puts Foo.who_self
puts Bar.greet
