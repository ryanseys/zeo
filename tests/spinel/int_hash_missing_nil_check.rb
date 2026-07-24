# Consumption-site nil-awareness for int-valued hashes (#801): a missing
# key is nil in Ruby. The read stays typed `int` (making it int? cascades
# through the self-host compiler), but at direct nil-check sites the
# missing key is reported via has_key. Covers `.nil?`, and the already-
# working `== nil` / `|| fallback`, across str_int / sym_int / int_int.

si = {a: 1, b: 2}
puts si[:zzz].nil?       # true
puts si[:a].nil?         # false
puts (si[:zzz] == nil)   # true
puts (si[:zzz] || 99)    # 99
puts (si[:a] || 99)      # 1

sti = {"x" => 1}
puts sti["y"].nil?       # true
puts sti["x"].nil?       # false
puts (sti["y"] || 7)     # 7

ii = {1 => 10, 2 => 20}
puts ii[9].nil?          # true
puts ii[1].nil?          # false
puts (ii[9] == nil)      # true
puts (ii[9] || 5)        # 5

# Present keys still behave as plain ints
puts (si[:a] + 100)      # 101
puts ii[2]               # 20
