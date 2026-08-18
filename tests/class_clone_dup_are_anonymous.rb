# `Module#dup`/`#clone` on a CLASS return the SAME OBJECT in zeo --
# `K.dup.equal?(K)` is true where ruby answers false. So the name is not
# merely wrong, it is `K`'s own: renaming the copy would rename `K`.
#
# `Kernel#dup`/`#clone` (crates/zeo-rt/src/builtins/kernel.rs) route a MODULE
# receiver to `runtime_meta::runtime_module_dup` and a CLASS receiver to
# `recv.dup_value(false)`, which for a `RubyValue::Class` is a handle copy.
# The comment there calls it "the documented handle-passthrough: copying one
# means copying its instances' layout, which this AOT model has no way to
# mint".
#
# That reason is now weaker than it reads, and this is the part worth
# knowing before anyone re-scopes it: `Class.new(K)` ALREADY mints a working
# anonymous runtime class -- verified, it answers `name == nil` and
# `superclass == K`. So the layout problem is solved; what is missing is the
# COPY. CRuby's `rb_mod_init_copy` moves the source's own method table,
# constants and class-level ivars onto the new class (`clone` additionally
# copies the singleton class and the frozen state; `dup` takes neither).
#
# A fix therefore looks like: mint through the same path `Class.new(K)` uses,
# then copy `K`'s own rows rather than inheriting them. The risk to watch is
# that zeo's compiled methods are registered per ClassId, so "copy the own
# rows" means registering the same fn pointers under the new id -- which the
# runtime overlay can already express (`define_method_own`), but which no
# caller does in bulk today.
class K; end
p K.clone.name
p K.dup.name
p K.name
