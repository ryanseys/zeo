# Issue #893: Process.pid / Process.ppid dispatched via getpid /
# getppid. Values vary by run; test the type. ppid is 0 on
# Windows where MinGW lacks getppid, >= 0 elsewhere.
puts Process.pid > 0
puts Process.ppid >= 0
