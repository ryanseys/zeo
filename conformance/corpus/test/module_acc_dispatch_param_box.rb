# Issue #314 follow-up: `Mod.accessor.method(args)` (the
# module-singleton-accessor dispatch from #126/#304) emitted
# call-site args with raw `compile_expr` (no boxing). When the
# resolved candidate's per-arg ptype was widened to `poly` (e.g.
# via #304's cross-class unification), but the caller passed an
# int/string at the specific call site, the C arg type didn't
# match the ptype slot — Wint-conversion / hard
# incompatible-pointer-conversion.
#
# Fix: in compile_call_expr's module-accessor branch, look up
# the resolved candidate's ptypes and box each arg through
# box_expr_to_poly when the target ptype is poly.
#
# Companion: scan_new_calls' #304 widening also gained a
# parallel branch for module candidates (`<Mod>_cls_<m>` stored
# in @meth_*) so the per-class unify reaches them.
#
# Surfaced via Roundhouse's `ActiveRecord.adapter.update(table,
# @id, attrs)` — `@id` widened to poly across heterogeneous
# subclasses, but the call site emitted `lv_id` (sp_RbVal) into
# `InMemoryAdapter.update`'s pre-widening `mrb_int lv_id` slot.

module Backend
  class << self
    attr_accessor :db
  end
end

module Storage
  module_function
  def fetch(id)
    # Just round-trip the id through to_s + length so the
    # method body works for both int and String inputs.
    id.to_s.length
  end
end

Backend.db = Storage

# Two call sites with different types — without #304 widening
# reaching the module candidate, only the first caller's type
# pinned the ptype; the second hit a C arg-type mismatch. With
# the widening + boxing fix, the ptype unifies to poly and both
# call sites box their arg through sp_box_int / sp_box_str.
puts Backend.db.fetch(42)          # 2
puts Backend.db.fetch("alpha")     # 5
