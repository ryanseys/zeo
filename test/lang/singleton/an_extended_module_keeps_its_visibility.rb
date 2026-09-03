# `obj.extend(M)` copies M's instance methods onto the object's singleton and
# LOSES their visibility marks. A `module_function` name is private on the
# instance side, so CRuby refuses `obj.extend(MF).public_send(:helper)` and
# zeo allows it.
#
# The cause is a missing dimension, not a wrong rule. The per-object
# singleton table is `singletons: FMap<usize, FMap<Symbol, MethodImpl>>`
# (`runtime_meta/mod.rs:239`) -- a name and a body, with nowhere to record
# whether the row is public, protected or private. `extend_object_default`
# (`runtime_meta/api.rs:1718`) copies through `module_extendable_method_names`
# and has nothing to copy the mark into, and `instance_method_visibility` has
# nothing to read.
#
# The class-receiver twin is already right: `class C; extend M; end` puts M in
# the singleton chain, where `class_method_is_private` walks real ancestors.
# Only the per-object copy is flat.
#
# The fix is a parallel `singleton_vis` map written by `extend_object_default`
# (from the source module's per-name visibility) and by
# `runtime_define_singleton_method` (from the visibility cursor), read by
# `instance_method_visibility` and by the `singleton_methods` /
# `public_methods` / `private_methods` reflection trio. It is the same shape
# the memory note calls "the per-object `extend` COPY that no overlay write
# reaches".
#
# Found by widening `a_module_function_is_public_on_the_singleton.rb`, which
# covers every route that IS right.

module MF
  module_function
  def helper = :helped
end

o = Object.new.extend(MF)
begin
  p o.public_send(:helper)
rescue NoMethodError => e
  p [:raised, e.class]
end
p o.send(:helper)
p o.singleton_methods.include?(:helper)
p o.public_methods(false).include?(:helper)
p o.private_methods(false).include?(:helper)
__END__
[:raised, NoMethodError]
:helped
false
false
true
