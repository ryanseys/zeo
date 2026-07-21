# Bundled tests:
#   - subclass_inherited_attr_reader
#   - subclass_override_param_propagate

# === subclass_inherited_attr_reader ===
# #508. Bare `attr_reader_name` and `self.attr_reader_name`
# inside a subclass instance method failed to resolve when the
# reader was declared via `attr_accessor` / `attr_reader` on the
# parent class. Codegen's attr_reader lookup only consulted
# @cls_attr_readers[current_class] and didn't walk the parent
# chain.
#
# Fix: swap the per-class loop for a call to
# `cls_has_attr_reader(ci, mname)`, which already walks
# @cls_parents recursively. Two emit sites: the bare-name path
# in compile_call_expr (instance method body, no receiver) and
# the obj-receiver path in compile_object_method_expr (the
# `self.fmt` shape).

class T_subclass_inherited_attr_reader_Base
  attr_accessor :fmt
  attr_reader :tag
  def initialize
    @fmt = :html
    @tag = :base_tag
  end
end

class T_subclass_inherited_attr_reader_Sub < T_subclass_inherited_attr_reader_Base
  def doit
    fmt == :html ? "html" : "json"
  end
  def doit_self
    self.fmt == :html ? "html-self" : "json-self"
  end
  def tag_str
    tag.to_s
  end
end

s = T_subclass_inherited_attr_reader_Sub.new
puts s.doit
puts s.doit_self
puts s.tag_str

# Deeper chain: grandchild via T_subclass_inherited_attr_reader_Mid -> T_subclass_inherited_attr_reader_Base.
class T_subclass_inherited_attr_reader_Mid < T_subclass_inherited_attr_reader_Base
end

class T_subclass_inherited_attr_reader_Leaf < T_subclass_inherited_attr_reader_Mid
  def show
    fmt.to_s + "/" + tag.to_s
  end
end

puts T_subclass_inherited_attr_reader_Leaf.new.show

# === subclass_override_param_propagate ===
# #516. When a parent-class instance method calls another method
# via implicit-self dispatch and a subclass overrides that
# method, the parent's typed call-site arg didn't propagate to
# the subclass override's parameter. The override's param
# stayed at the int default and the body's dispatch on it
# failed.
#
# Fix: in scan_cls_method_calls' implicit-self CallNode arm,
# after widening the same-class method's ptypes from the
# arg_ids, also walk @cls_names for descendants that
# direct-override the same method name and widen their
# ptypes from the same arg_ids. The descendant's C signature
# now agrees with the cls_id-switch dispatch that the imeth
# emit later builds.

class T_subclass_override_param_propagate_Base
  def reload(row)
    consume(row)
  end
  def consume(_row)
    "base default"
  end
end

class T_subclass_override_param_propagate_Sub < T_subclass_override_param_propagate_Base
  def consume(row)
    row["id"]
  end
end

puts T_subclass_override_param_propagate_Sub.new.reload({"id" => "42"})

# Three-level chain: param flows through two override layers.
class T_subclass_override_param_propagate_Parent
  def kick(payload)
    handle(payload)
  end
  def handle(_p)
    "parent"
  end
end

class T_subclass_override_param_propagate_Mid < T_subclass_override_param_propagate_Parent
  def handle(p)
    "mid:" + p[:name]
  end
end

class T_subclass_override_param_propagate_Leaf < T_subclass_override_param_propagate_Mid
  def handle(p)
    "leaf:" + p[:name]
  end
end

puts T_subclass_override_param_propagate_Mid.new.kick({name: "m"})
puts T_subclass_override_param_propagate_Leaf.new.kick({name: "l"})

