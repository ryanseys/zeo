# Bundled tests:
#   - inherit
#   - inherited_cmeth_self_returning_local
#   - inherited_cmethod_subclass_dispatch

# === inherit ===
# Test inheritance and super

class T_inherit_Animal
  def initialize(name)
    @name = name
  end

  def name
    @name
  end

  def speak
    "..."
  end

  def describe
    name
  end
end

class T_inherit_Dog < T_inherit_Animal
  def initialize(name, breed)
    super(name)
    @breed = breed
  end

  def breed
    @breed
  end

  def speak
    "Woof!"
  end
end

class T_inherit_Cat < T_inherit_Animal
  def speak
    "Meow!"
  end
end

# Basic inheritance
dog = T_inherit_Dog.new("Rex", "Labrador")
puts dog.name       # Rex (inherited)
puts dog.breed      # Labrador
puts dog.speak      # Woof! (overridden)
puts dog.describe   # Rex (inherited, calls name)

cat = T_inherit_Cat.new("Whiskers")
puts cat.name       # Whiskers (inherited)
puts cat.speak      # Meow! (overridden)
puts cat.describe   # Whiskers (inherited)

# === inherited_cmeth_self_returning_local ===
# Inherited class method whose body holds a local typed by a
# method that each subclass overrides. The local's C type
# needs to specialize per-subclass.
#
# Pre-fix: scan_locals stored the result in @nd_scope_names
# keyed by the shared AST body id; whichever subclass scanned
# last won, leaving every other subclass with the wrong LV
# C type. Surfaced in real-blog's `T_inherited_cmeth_self_returning_local_Comment.find` returning
# `sp_Article *` (warning under -Wincompatible-pointer-types,
# silent miscompile if T_inherited_cmeth_self_returning_local_Article / T_inherited_cmeth_self_returning_local_Comment struct layouts
# diverge).
#
# Fix: per-(class, cmeth_idx) scope tables in
# @cls_cmeth_scope_names / @cls_cmeth_scope_types preserve
# each subclass's scan result. Codegen consumes the per-
# subclass entry instead of the per-bid one.

class T_inherited_cmeth_self_returning_local_Base
  def self.find(id)
    result = adapter_find(id)
    result
  end
end

class T_inherited_cmeth_self_returning_local_Article < T_inherited_cmeth_self_returning_local_Base
  attr_accessor :id, :title
  def self.adapter_find(id)
    a = T_inherited_cmeth_self_returning_local_Article.new
    a.id = id
    a.title = "art-#{id}"
    a
  end
end

class T_inherited_cmeth_self_returning_local_Comment < T_inherited_cmeth_self_returning_local_Base
  attr_accessor :id, :body
  def self.adapter_find(id)
    c = T_inherited_cmeth_self_returning_local_Comment.new
    c.id = id
    c.body = "comment-#{id}"
    c
  end
end

a = T_inherited_cmeth_self_returning_local_Article.find(7)
puts a.title
c = T_inherited_cmeth_self_returning_local_Comment.find(9)
puts c.body

# === inherited_cmethod_subclass_dispatch ===
# #523. Sibling to #516. When a subclass inherits a class method
# whose body calls another class method (`self.last` calls bare
# `all`), spinel correctly monomorphized the call (`sp_Sub_cls_all`
# is invoked) but the surrounding inference of the call's result
# type used the parent's signature -- the local slot for the
# returned array was typed `sp_IntArray *` (from T_inherited_cmethod_subclass_dispatch_Base.all's empty
# `[]` literal) while `sp_Sub_cls_all` returned `sp_PtrArray *`,
# triggering an `incompatible pointer types` C warning and an
# `int-from-pointer` error on the subsequent `[-1]` dispatch.
#
# Root cause: analyze's walk_and_cache wrote a per-AST-node-id
# type cache, and inherited cmethod bodies share their AST node
# ids across subclass copies. The first walker (typically the
# defining class) populated the cache; later walkers' recomputes
# under different @current_class_idx were short-circuited by the
# cache hit at the top of infer_type.
#
# Fix (analyze): walk_and_cache invalidates the cache before
# recompute so the second walker overwrites, and skip_cache is
# extended to bare CallNodes that resolve to a sibling cmeth on
# the current class. Fix (codegen): infer_type's CallNode arm now
# mirrors analyze's @cls_cmeth_returns lookup for bare cmeth
# calls so the cache-miss path picks up the subclass's override
# at emit time.

class T_inherited_cmethod_subclass_dispatch_Base
  def self.all
    []
  end

  def self.last
    records = all
    records.empty? ? nil : records[-1]
  end
end

class T_inherited_cmethod_subclass_dispatch_Sub < T_inherited_cmethod_subclass_dispatch_Base
  def self.all
    [Object.new]
  end
end

# T_inherited_cmethod_subclass_dispatch_Sub.last (inherited from T_inherited_cmethod_subclass_dispatch_Base) sees T_inherited_cmethod_subclass_dispatch_Sub.all's PtrArray return
# type now, so `records.empty?` and `records[-1]` dispatch into
# the PtrArray path instead of the IntArray path.
r = T_inherited_cmethod_subclass_dispatch_Sub.last
if r != nil
  puts "got object"
else
  puts "nil"
end

