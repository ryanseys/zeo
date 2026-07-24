# Global variable compound writes — Or / And / Operator / Target.
# Was four tests; merged here. Globals across the original files
# happened to reuse `$count`, so this file picks distinct names per
# section so the merged script's accumulated state doesn't change
# the per-section semantics.

# === Operator-write ($x += / -= / *= / /= ... ) ===
# Mirror of LocalVariableOperatorWriteNode but the storage is a
# C-level global symbol via sanitize_gvar(name).
$counter = 0
$counter += 1
$counter += 5
puts $counter   # 6
$counter -= 2
puts $counter   # 4
$counter *= 3
puts $counter   # 12
$counter /= 4
puts $counter   # 3

# === Or-write ($x ||= val) ===
# Mirror of LocalVariableOrWriteNode. Only assigns if the global
# is currently falsy.
#
# Note: Spinel uses C-truthy semantics (any zero is falsy), which
# diverges from CRuby (only nil/false are falsy). For unassigned
# globals (default 0/nil) the two agree on the first assignment;
# subsequent ||= against truthy non-zero ints also agree. This
# test exercises only the agreement region.
$or_count ||= 5    # never assigned, fires
puts $or_count     # 5
$or_count ||= 99   # already 5 (truthy in both), doesn't fire
puts $or_count     # 5

# === And-write ($x &&= val) ===
# Mirror of LocalVariableAndWriteNode. Only assigns if the global
# is currently truthy.
$and_count = 5
$and_count &&= 10  # 5 is truthy, fires
puts $and_count    # 10
$and_count &&= 0   # 10 is truthy in C, fires
puts $and_count    # 0

# === Target (multi-assign LHS for globals) ===
# `$a, $b = 1, 2` -- each LHS slot is a GlobalVariableTargetNode.
# Routes through MultiWriteNode's emit_multi_write_target to the
# same g_<name> C-level storage GlobalVariableWriteNode uses.
$ga, $gb = 1, 2
puts $ga          # 1
puts $gb          # 2
$ga, $gb = $gb, $ga    # swap
puts $ga          # 2
puts $gb          # 1
