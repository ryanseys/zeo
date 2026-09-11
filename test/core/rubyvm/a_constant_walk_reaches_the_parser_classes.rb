# A walk of the constant tree reaches the parser-backed classes under the VM
# namespace without the program ever writing that namespace's name, and each
# class it reaches answers its own methods.
vm = Object.const_get(Object.constants.find { |c| c.to_s == "Ruby" + "VM" })
vm.constants.sort.each do |c|
  v = vm.const_get(c)
  next unless v.is_a?(Module)
  puts "#{v}: #{v.singleton_methods(false).sort.first(3).inspect}"
end
__END__
RubyVM::AbstractSyntaxTree: [:node_id_for_backtrace_location, :of, :parse]
RubyVM::InstructionSequence: [:compile, :compile_file, :compile_file_prism]
RubyVM::YJIT: [:code_gc, :disasm, :dump_exit_locations]
