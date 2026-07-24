# Bundled tests:
#   - fiber_storage_inherits
#   - fiber_storage_persists
#   - fiber_storage_siblings

# === fiber_storage_inherits ===
def t_fiber_storage_inherits
  # Parent fiber's storage is snapshot-copied into a child fiber at
  # Fiber.new time. Subsequent writes on either side are independent
  # (shallow copy).
  Fiber[:user] = "alice"
  
  f = Fiber.new do
    # Inherited at creation.
    puts Fiber[:user]
    # Local write — does not leak back to parent.
    Fiber[:user] = "bob"
    puts Fiber[:user]
  end
  f.resume
  
  # Parent's value is unchanged by the child's write.
  puts Fiber[:user]
end
t_fiber_storage_inherits

# === fiber_storage_persists ===
def t_fiber_storage_persists
  # Storage values persist across yield/resume cycles within the same
  # fiber. Mirrors the existing fiber_ivar_persists_across_yield test
  # but exercises the storage path instead of ivars.
  f = Fiber.new do
    Fiber[:n] = 1
    Fiber.yield
    # After resume, the previous storage write is still visible.
    Fiber[:n] = Fiber[:n] + 1
    Fiber.yield
    Fiber[:n] = Fiber[:n] + 1
    puts Fiber[:n]
  end
  
  f.resume   # sets :n = 1
  f.resume   # bumps to 2, yields
  f.resume   # bumps to 3, prints 3
end
t_fiber_storage_persists

# === fiber_storage_siblings ===
def t_fiber_storage_siblings
  # Sibling fibers have independent storage. A write in one does not
  # affect the other even if they share a parent.
  Fiber[:tag] = "parent"
  
  a = Fiber.new do
    Fiber[:tag] = "a"
    puts Fiber[:tag]
  end
  
  b = Fiber.new do
    # b was created BEFORE a ran, so it inherited "parent", not "a".
    puts Fiber[:tag]
    Fiber[:tag] = "b"
    puts Fiber[:tag]
  end
  
  a.resume
  b.resume
  
  # Parent's storage still reads "parent" — neither child leaked back.
  puts Fiber[:tag]
end
t_fiber_storage_siblings

