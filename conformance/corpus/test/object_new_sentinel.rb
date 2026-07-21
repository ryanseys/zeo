# Issue #63: `Object.new.freeze` constants used as identity sentinels
# generated `sp_Object *` declarations against a type that didn't exist
# in the runtime, plus the `freeze` call fell through the dispatcher
# and the constant was assigned `0` (NULL) instead of the new object.
# After the fix:
#   - lib/sp_runtime.h defines sp_Object and sp_Object_new (a fresh
#     GC-managed allocation per call so `==` is identity)
#   - codegen emits sp_Object_new() for `Object.new`
#   - the receiver-passthrough `freeze` arm in compile_object_method_expr
#     keeps the chain alive so the constant gets the actual pointer.

NO_DIRECT_CALL = Object.new.freeze
HASH_MISS = Object.new.freeze

# Distinct sentinels: their addresses differ.
puts NO_DIRECT_CALL == HASH_MISS         # false
puts NO_DIRECT_CALL == NO_DIRECT_CALL    # true
puts HASH_MISS == HASH_MISS              # true
