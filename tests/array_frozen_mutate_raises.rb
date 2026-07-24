# Array mutating methods on frozen arrays must raise FrozenError.
# IntArray#pop/#shift already had the guard; FloatArray, StrArray,
# and the reverse_bang/sort_bang/shuffle_bang/rotate_bang/delete_at
# /delete/insert methods were missing it.

# IntArray
a = [1, 2, 3]
a.freeze
begin; a.reverse!; puts "BUG int reverse!"; rescue FrozenError; puts "int reverse!"; end
begin; a.sort!; puts "BUG int sort!"; rescue FrozenError; puts "int sort!"; end
begin; a.shuffle!; puts "BUG int shuffle!"; rescue FrozenError; puts "int shuffle!"; end
begin; a.rotate!(1); puts "BUG int rotate!"; rescue FrozenError; puts "int rotate!"; end
begin; a.delete_at(0); puts "BUG int delete_at"; rescue FrozenError; puts "int delete_at"; end
begin; a.delete(1); puts "BUG int delete"; rescue FrozenError; puts "int delete"; end

# FloatArray
f = [1.5, 2.5, 3.5]
f.freeze
begin; f.reverse!; puts "BUG float reverse!"; rescue FrozenError; puts "float reverse!"; end
begin; f.sort!; puts "BUG float sort!"; rescue FrozenError; puts "float sort!"; end
begin; f.shuffle!; puts "BUG float shuffle!"; rescue FrozenError; puts "float shuffle!"; end
begin; f.rotate!(1); puts "BUG float rotate!"; rescue FrozenError; puts "float rotate!"; end
begin; f.pop; puts "BUG float pop"; rescue FrozenError; puts "float pop"; end
begin; f.shift; puts "BUG float shift"; rescue FrozenError; puts "float shift"; end

# StrArray
s = ["a", "b", "c"]
s.freeze
begin; s.reverse!; puts "BUG str reverse!"; rescue FrozenError; puts "str reverse!"; end
begin; s.sort!; puts "BUG str sort!"; rescue FrozenError; puts "str sort!"; end
begin; s.shuffle!; puts "BUG str shuffle!"; rescue FrozenError; puts "str shuffle!"; end
begin; s.rotate!(1); puts "BUG str rotate!"; rescue FrozenError; puts "str rotate!"; end
begin; s.pop; puts "BUG str pop"; rescue FrozenError; puts "str pop"; end
begin; s.delete_at(0); puts "BUG str delete_at"; rescue FrozenError; puts "str delete_at"; end
begin; s.delete("a"); puts "BUG str delete"; rescue FrozenError; puts "str delete"; end
begin; s.insert(0, "z"); puts "BUG str insert"; rescue FrozenError; puts "str insert"; end
