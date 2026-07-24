# Bundled tests:
#   - poly_array_map
#   - poly_array_push_handlers
#   - poly_hash
#   - primitive_is_a

# === poly_array_map ===
def t_poly_array_map
  # `compile_map_expr` had no `poly_array` recv branch — a
  # heterogeneous-element array (`[1, "two", :three]`) called
  # `.map { ... }` and the dispatch fell through to the `"0"`
  # placeholder. The result was typed as `sp_IntArray *` (per the
  # inferred map result), `lv_out = 0` was emitted, and any
  # `.length` / `[i]` on the accumulator dereferenced NULL,
  # crashing at runtime.
  
  # (1) poly_array → IntArray (int-return block).
  arr = [1, "two", :three, 4.5]
  out = arr.map { |x| x.to_s.length }
  puts out.length      # 4
  puts out[0]          # 1
  puts out[1]          # 3
  puts out[2]          # 5
  puts out[3]          # 3
  
  # (2) poly_array → StrArray (string-return block).
  strs = arr.map { |x| x.to_s }
  puts strs.length     # 4
  puts strs[0]         # 1
  puts strs[1]         # two
  puts strs[2]         # three
  puts strs[3]         # 4.5
  
  # (3) poly_array → IntArray (different int expression).
  sizes = arr.map { |x| x.to_s.length * 2 }
  puts sizes.length    # 4
  puts sizes[0]        # 2
  puts sizes[3]        # 6
end
t_poly_array_map

# === poly_array_push_handlers ===
def t_poly_array_push_handlers
  # `compile_call_expr` and `compile_mutating_call_stmt` had no `<<`
  # / `push` arms for poly_array recv. The expr-context fall-through
  # produced raw C `<<` on `sp_PolyArray *` (gcc error: "invalid
  # operands to binary <<"); the stmt-context fall-through silently
  # dropped the push from generated C entirely.
  #
  # Repro: a heterogeneous literal lowers to poly_array; then both
  # `arr << v` (expr / stmt-without-receiver-rewrite) and
  # `arr.push(v)` exercise the dispatch.
  
  arr = [1, "two", :three]
  arr << 42
  arr.push("four")
  puts arr.length
  puts arr[0]
  puts arr[1]
  puts arr[2]
  puts arr[3]
  puts arr[4]
end
t_poly_array_push_handlers

# === poly_hash ===
def t_poly_hash
  # Test heterogeneous Hash
  
  h = {name: "Alice", age: 30, active: true}
  puts h[:name]     # Alice
  puts h[:age]      # 30
  puts h[:active]   # true
  puts h.length     # 3
  
  # Iteration
  h.each do |k, v|
    puts k
  end
  # name, age, active
  
  puts "done"
end
t_poly_hash

# === primitive_is_a ===
def t_primitive_is_a
  # is_a? / kind_of? / instance_of? on primitive receivers — answer
  # decided at compile time based on Ruby's class hierarchy.
  
  # === Integer ===
  p 5.is_a?(Integer)
  p 5.is_a?(Numeric)
  p 5.is_a?(Comparable)
  p 5.is_a?(Object)
  p 5.is_a?(Float)
  p 5.is_a?(String)
  p 5.kind_of?(Integer)
  p 5.kind_of?(Numeric)
  p 5.instance_of?(Integer)
  p 5.instance_of?(Numeric)   # false — instance_of? doesn't follow superclass
  
  # === Float ===
  p 1.5.is_a?(Float)
  p 1.5.is_a?(Numeric)
  p 1.5.is_a?(Comparable)
  p 1.5.is_a?(Integer)
  p 1.5.kind_of?(Numeric)
  p 1.5.instance_of?(Float)
  p 1.5.instance_of?(Numeric)
  
  # === String ===
  p "hi".is_a?(String)
  p "hi".is_a?(Comparable)
  p "hi".is_a?(Object)
  p "hi".is_a?(Integer)
  p "hi".kind_of?(String)
  p "hi".instance_of?(String)
  p "hi".instance_of?(Comparable)
  
  # === Symbol ===
  p :hi.is_a?(Symbol)
  p :hi.is_a?(Comparable)
  p :hi.is_a?(String)
  p :hi.kind_of?(Symbol)
  p :hi.instance_of?(Symbol)
  
  # === Top-level ancestors (Kernel / BasicObject) ===
  # Every value is in the Object hierarchy, so is_a? returns true for
  # Kernel and BasicObject as well as Object. instance_of? still
  # returns false for these because they're ancestors, not the exact
  # class of the receiver.
  p 5.is_a?(Kernel)
  p 5.is_a?(BasicObject)
  p 1.5.is_a?(Kernel)
  p 1.5.is_a?(BasicObject)
  p "hi".is_a?(Kernel)
  p "hi".is_a?(BasicObject)
  p :hi.is_a?(Kernel)
  p :hi.is_a?(BasicObject)
  p 5.instance_of?(Kernel)
  p 5.instance_of?(BasicObject)
end
t_primitive_is_a

