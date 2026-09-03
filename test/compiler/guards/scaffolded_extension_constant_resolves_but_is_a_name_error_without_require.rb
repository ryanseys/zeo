# A scaffolded ext (json) is still require-gated: its constant is a
# NameError until `require "json"` fires, exactly like the fully-built
# ones -- scaffolding changes only what the METHODS do, not the gate.

puts JSON.generate([1])
__END__
#@ stderr
compiler/guards/scaffolded_extension_constant_resolves_but_is_a_name_error_without_require.rb:5:in '<main>': uninitialized constant JSON (NameError)
#@ exit 1
