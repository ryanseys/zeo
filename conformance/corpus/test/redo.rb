# RedoNode coverage — `redo` re-runs the current iteration of the
# enclosing loop without re-evaluating the guard or advancing the
# iterator. Spinel emits a labeled goto back to the iteration top.
# Each loop form needs the wrapping helper trio (push_redo_label /
# emit_redo_label / pop_redo_label) so this file exercises every
# form that has it.
#
# Per-section `def t_<form>; ...; end; t_<form>` isolates the
# `attempts` / `i` / `x` locals. Was 17 separate tests covering
# each loop form individually plus a nested case.

# === times ===
def t_times
  attempts = 0
  3.times do |i|
    attempts += 1
    if i == 1 && attempts < 5
      redo
    end
    puts "i=#{i} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_times

# === Array#each ===
def t_each
  attempts = 0
  [10, 20, 30].each do |x|
    attempts += 1
    if x == 20 && attempts < 5
      redo
    end
    puts "x=#{x} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_each

# === while ===
def t_while
  i = 0
  attempts = 0
  while i < 3
    attempts += 1
    if i == 1 && attempts < 5
      redo
    end
    puts "i=#{i} attempt=#{attempts}"
    i += 1
  end
  puts "total=#{attempts}"
end
t_while

# === until ===
def t_until
  i = 0
  attempts = 0
  until i >= 3
    attempts += 1
    if i == 1 && attempts < 5
      redo
    end
    puts "i=#{i} attempt=#{attempts}"
    i += 1
  end
  puts "total=#{attempts}"
end
t_until

# === for-in over range ===
def t_for_range
  attempts = 0
  for i in 0..2
    attempts += 1
    if i == 1 && attempts < 5
      redo
    end
    puts "i=#{i} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_for_range

# === for-in over array ===
def t_for_array
  attempts = 0
  for x in [10, 20, 30]
    attempts += 1
    if x == 20 && attempts < 5
      redo
    end
    puts "x=#{x} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_for_array

# === Kernel#loop (uses break to terminate) ===
def t_loop
  i = 0
  attempts = 0
  loop do
    attempts += 1
    if i == 1 && attempts < 5
      redo
    end
    puts "i=#{i} attempt=#{attempts}"
    i += 1
    break if i >= 3
  end
  puts "total=#{attempts}"
end
t_loop

# === Integer#upto ===
def t_upto
  attempts = 0
  0.upto(2) do |i|
    attempts += 1
    if i == 1 && attempts < 5
      redo
    end
    puts "i=#{i} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_upto

# === Integer#downto ===
def t_downto
  attempts = 0
  2.downto(0) do |i|
    attempts += 1
    if i == 1 && attempts < 5
      redo
    end
    puts "i=#{i} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_downto

# === Integer#step ===
def t_step
  attempts = 0
  0.step(4, 2) do |i|
    attempts += 1
    if i == 2 && attempts < 5
      redo
    end
    puts "i=#{i} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_step

# === Array#each_with_index ===
def t_each_with_index
  attempts = 0
  ["a", "b", "c"].each_with_index do |x, i|
    attempts += 1
    if i == 1 && attempts < 5
      redo
    end
    puts "x=#{x} i=#{i} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_each_with_index

# === Array#each_cons ===
def t_each_cons
  attempts = 0
  [1, 2, 3, 4].each_cons(2) do |pair|
    attempts += 1
    if pair[0] == 2 && attempts < 5
      redo
    end
    puts "pair=#{pair.inspect} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_each_cons

# === Array#each_slice ===
def t_each_slice
  attempts = 0
  [1, 2, 3, 4, 5].each_slice(2) do |chunk|
    attempts += 1
    if chunk[0] == 3 && attempts < 5
      redo
    end
    puts "chunk=#{chunk.inspect} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_each_slice

# === Array#each_with_object ===
def t_each_with_object
  attempts = 0
  result = [1, 2, 3].each_with_object([]) do |x, acc|
    attempts += 1
    if x == 2 && attempts < 5
      redo
    end
    acc << x * 10
  end
  puts result.inspect
  puts "total=#{attempts}"
end
t_each_with_object

# === Array#zip iteration ===
def t_zip
  attempts = 0
  [1, 2, 3].zip([10, 20, 30]) do |a, b|
    attempts += 1
    if a == 2 && attempts < 5
      redo
    end
    puts "a=#{a} b=#{b} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_zip

# === Array#cycle (with count to terminate) ===
def t_cycle
  attempts = 0
  [1, 2].cycle(2) do |x|
    attempts += 1
    if x == 1 && attempts < 3
      redo
    end
    puts "x=#{x} attempt=#{attempts}"
  end
  puts "total=#{attempts}"
end
t_cycle

# === Nested redo (mixed loop kinds) — exercises label-stack uniqueness ===
def t_nested
  inner_attempts = 0
  outer_done = 0
  i = 0
  while i < 2
    outer_done += 1
    3.times do |j|
      inner_attempts += 1
      if i == 0 && j == 1 && inner_attempts < 4
        redo
      end
      puts "i=#{i} j=#{j} attempt=#{inner_attempts}"
    end
    i += 1
  end
  puts "outer_done=#{outer_done}"
  puts "inner_attempts=#{inner_attempts}"
end
t_nested
