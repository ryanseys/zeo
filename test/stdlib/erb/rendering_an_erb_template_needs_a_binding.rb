# `ERB#result` and `#result_with_hash` evaluate the compiled source in a
# Binding derived from TOPLEVEL_BINDING.
require "erb"

p ERB.new("<%= 1 + 1 %> and <%= name %>").result_with_hash(name: "zeo")
p ERB.new("<%= 2 * 3 %>").result(binding)
__END__
"2 and zeo"
"6"
