# AliasGlobalVariableNode -- `alias $copy $orig`.
#
# Compile-time only: Spinel registers $copy as a name pointing to
# $orig's storage slot. Subsequent reads/writes of either share
# state. Out of scope: dynamic alias from method body, since the
# entire compile-time map is built once at AST scan.

$orig = "hello"
alias $copy $orig
puts $copy        # hello

$orig = "updated"
puts $copy        # updated

$copy = "back"
puts $orig        # back
