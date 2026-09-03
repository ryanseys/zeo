# Top-level `def` in ANY file (main or required) is a separate,
# pre-existing gap ("unexpected top-level-only node"), routed
# around with classes/module functions here exactly like every
# prior phase's tests.

require_relative "require_relative_splices_in_document_order_and_shares_the_global_namespace/main"
__END__
before require
greeter loaded
after require
hello, world!
hello
