# The empty symbol literal `:""` is a real SymbolNode whose content is
# "". collect_sym_names skipped "" (its shared helper guards param-name
# callers against a blank), so the symbol got no table entry and
# compile_symbol_literal fell back to sp_sym_intern -- a function that is
# only emitted when the intern path is otherwise needed, leaving an
# `undefined reference to sp_sym_intern` link failure for a bare `:""`.

s = :""
puts s.to_s.length          #=> 0
puts(s == :"")              #=> true
puts(:"" == :"")           #=> true
puts(:"" == :a)            #=> false

# In a symbol array and as a value.
arr = [:"", :a, :""]
puts arr.length             #=> 3
puts arr[0].to_s.length     #=> 0
puts arr[1].to_s            #=> a

# Round-trips through to_s/to_sym.
t = "".to_sym
puts(t == :"")             #=> true
puts "done"
