# AliasGlobalVariableNode -- `alias $copy $orig`.
#
# $copy becomes another name for $orig's storage, so reads and writes of
# either are seen through the other. Out of scope: a dynamic alias from a
# method body, since the
# entire compile-time map is built once at AST scan.

$orig = "hello"
alias $copy $orig
puts $copy        # hello

$orig = "updated"
puts $copy        # updated

$copy = "back"
puts $orig        # back
__END__
hello
updated
back
