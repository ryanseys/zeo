# #564 (Ori Pekelman). Fiber.new block bodies that capture a
# local push a `_hcell_<name>_<n>` entry onto
# @heap_promoted_names so the capturing function's emit sees
# `(*_hcell_<name>_<n>)` instead of `lv_<name>`. The list was
# never reset at method boundaries, so a sibling method
# defined later in the same compilation unit (e.g. a top-level
# `def do_work(client)` called from inside the fiber body)
# also saw `(*_hcell_<name>_<n>)` in its own body emit and
# the link failed with `'_hcell_<name>_0' undeclared`.
#
# Fix: save / restore @heap_promoted_names length around each
# top-level method body emit so the cells only live in the
# C scope that actually allocates them.

def spawn_for(client)
  f = Fiber.new do
    do_work(client)
  end
  f
end

def do_work(client)
  puts "client=" + client.to_s
end

f = spawn_for(42)
f.resume
