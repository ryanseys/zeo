# Hash#fetch(key) { |k| ... } yields the missing key to the block.
# The block param must be typed as the hash's KEY type (sym / string /
# int), not the default int, so block-body uses like k.to_s / k + "!"
# dispatch correctly. Top-level calls exercise block_param_type_at;
# the in-method assignment forms exercise scan_locals.

# Top-level (block_param_type_at), distinct param names so each block
# gets its own scope slot.
puts({ 1 => 10 }.fetch(99) { |ka| ka * 100 })
puts({ a: 10 }.fetch(:zzz) { |kb| kb.to_s })
puts({ "a" => 10 }.fetch("zzz") { |kc| kc + "!" })

# In-method, value consumed via an assignment RHS (scan_locals).
def m_int
  r = { 1 => 10 }.fetch(99) { |k| k * 100 }
  r
end

def m_sym
  r = { a: 10 }.fetch(:zzz) { |k| k.to_s }
  r
end

def m_str
  r = { "a" => 10 }.fetch("zzz") { |k| k + "!" }
  r
end

def m_sym_concat
  r = { a: 10 }.fetch(:missing) { |k| "key=" + k.to_s }
  r
end

puts m_int
puts m_sym
puts m_str
puts m_sym_concat
