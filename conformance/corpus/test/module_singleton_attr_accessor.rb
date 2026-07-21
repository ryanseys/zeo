# #511. `module M; class << self; attr_accessor :x; end; end` on a
# module previously didn't recognize either the reader or writer
# for non-constant-RHS assignments — the existing constant-fold
# path (rewrite_module_singleton_accessors) only resolved when
# every observed write was a ConstantReadNode RHS. Roundhouse hit
# this with `ActiveRecord.adapter = SqliteAdapter.new` (the RHS
# is a method call, not a constant).
#
# Fix: when the SingletonClassNode + attr_accessor shape is seen,
# also synthesize a poly-typed file-scope const `<Mod>_<accessor>`.
# Reads of `Mod.accessor` and writes of `Mod.accessor = v` then
# fall through to the const slot when the constant-fold path is
# empty or poisoned. The const initializes to `{SP_TAG_NIL,...}`
# so an un-assigned read returns nil.

module Cfg
  class << self
    attr_accessor :adapter, :timeout
  end
end

# String RHS (non-constant): codegen falls through to the poly
# slot. Reader returns sp_RbVal -> sp_poly_puts.
Cfg.adapter = "sqlite"
puts Cfg.adapter

# Int RHS.
Cfg.timeout = 30
puts Cfg.timeout

# Re-assign.
Cfg.adapter = "postgres"
puts Cfg.adapter

# Pre-assignment read is nil.
module Empty
  class << self
    attr_accessor :slot
  end
end
puts Empty.slot.nil?
