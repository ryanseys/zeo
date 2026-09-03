# Process syscall-wrapper surface (batch 1): pgid/session queries, resource
# limits, and the non-effectful setters that return stable values. Output is
# written as relational/type assertions so it stays byte-identical to CRuby
# across machines rather than baking in platform-specific numbers.

# Process-group / session queries agree with the no-arg forms.
puts Process.getpgid(0) == Process.getpgrp

# getrlimit returns a [soft, hard] pair of Integers with soft <= hard
# (RLIM_INFINITY compares greater than any finite limit).
soft, hard = Process.getrlimit(Process::RLIMIT_NOFILE)
puts [soft, hard].all? { |v| v.is_a?(Integer) }
puts soft <= hard

# The RLIMIT_* / wait-flag constants are defined Integers.
puts [Process::RLIMIT_NOFILE, Process::RLIMIT_CPU, Process::WNOHANG,
      Process::WUNTRACED].all? { |c| c.is_a?(Integer) }

# RLIM_INFINITY is the saturating sentinel: greater than any real soft limit.
puts Process::RLIM_INFINITY >= hard

# warmup is a no-op hint that answers true.
puts Process.warmup

# setproctitle echoes the title string back.
puts(Process.setproctitle("zeo-batch1") == "zeo-batch1")

# maxgroups reads back a positive Integer, and a write is clamped by the OS
# ceiling (never larger than what we asked for) but stays positive.
Process.maxgroups = 8
puts Process.maxgroups.is_a?(Integer) && Process.maxgroups.positive?
puts Process.maxgroups <= 8

# The whole new surface answers respond_to? affirmatively.
puts %i[getpgid setpgid setpgrp setsid setpriority getrlimit setrlimit
        maxgroups warmup setproctitle initgroups daemon].all? { |m|
  Process.respond_to?(m)
}
__END__
true
true
true
true
true
true
true
true
true
true
