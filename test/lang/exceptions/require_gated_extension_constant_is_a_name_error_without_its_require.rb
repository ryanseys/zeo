# The ext require-gate: a require-gated builtin's constant
# (`Base64`, gated by `"base64"`) is INVISIBLE until `require "base64"`
# activates it -- referencing it un-required raises `NameError:
# uninitialized constant Base64`, oracle-verified against ruby 4.0.6
# (`uninitialized constant Base64 (NameError)`). The gate rides the
# ordinary `resolve_class` -> unset-constant path, so it's a rescuable
# RUNTIME NameError, not a compile error.

puts Base64.strict_encode64("hi")
__END__
#@ stderr
lang/exceptions/require_gated_extension_constant_is_a_name_error_without_its_require.rb:9:in '<main>': uninitialized constant Base64 (NameError)
#@ exit 1
