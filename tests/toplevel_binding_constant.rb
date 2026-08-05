# TOPLEVEL_BINDING is a global constant from boot: self = main, cref Object.
p Object.constants.include?(:TOPLEVEL_BINDING)
p TOPLEVEL_BINDING.class
p TOPLEVEL_BINDING.receiver.to_s

x = 42
p TOPLEVEL_BINDING.local_variable_defined?(:x)
p TOPLEVEL_BINDING.local_variable_get(:x)
