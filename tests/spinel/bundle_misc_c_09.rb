# Bundled tests:
#   - fiber_storage_basic
#   - fiber_storage_current_alias
#   - fiber_storage_gc_root

# === fiber_storage_basic ===
def t_fiber_storage_basic
  Fiber[:user_id] = 42
  puts Fiber[:user_id]
  
  Fiber[:user_id] = 43
  puts Fiber[:user_id]
  
  # Unset key reads as nil.
  v = Fiber[:not_set]
  if v.nil?
    puts "nil_ok"
  else
    puts "nil_unexpected"
  end
end
t_fiber_storage_basic

# === fiber_storage_current_alias ===
def t_fiber_storage_current_alias
  # `Fiber.current[:k]` and `Fiber.current[:k] = v` are aliases for
  # `Fiber[:k]` and `Fiber[:k] = v` — both target the same storage Hash
  # on the currently-running fiber.
  
  Fiber.current[:via_current] = 100
  puts Fiber[:via_current]                       # 100 — reads via Fiber[]
  
  Fiber[:via_bare] = 200
  puts Fiber.current[:via_bare]                  # 200 — reads via Fiber.current[]
end
t_fiber_storage_current_alias

# === fiber_storage_gc_root ===
def t_fiber_storage_gc_root
  # Regression: sp_fiber_root.storage must survive GC. sp_fiber_root
  # is a static (not sp_gc_alloc'd) so its scan never runs via the
  # heap walker. Without explicit marking in sp_re_mark_globals, the
  # SymPolyHash allocated by `Fiber[:k] = v` at top level gets
  # prematurely collected on the next cycle.
  #
  # Force GC by allocating many sp_gc_alloc'd objects (arrays of
  # strings, each becoming garbage on the next iteration) to push
  # sp_gc_bytes past the 256KB threshold. Then verify the storage
  # value still reads back.
  
  Fiber[:answer] = 42
  
  i = 0
  while i < 10000
    # Each iteration allocates a fresh PolyArray + a String element;
    # both are GC-tracked (sp_gc_alloc), so this drives sp_gc_bytes
    # past the auto-collect threshold within the loop.
    garbage = ["garbage_" + i.to_s, "more_" + i.to_s, "still_" + i.to_s]
    i = i + 1
  end
  
  # If sp_fiber_root.storage wasn't marked, the SymPolyHash holding
  # :answer would have been freed and read would return nil.
  v = Fiber[:answer]
  if v == 42
    puts "survived"
  else
    puts "lost"
  end
end
t_fiber_storage_gc_root

