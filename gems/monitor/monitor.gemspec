# The Ruby half of `monitor`: MonitorMixin. The reentrant lock itself is the
# statically linked `ext-monitor` module in spinel-rt -- the same split CRuby
# makes, where Monitor is a C core class (thread_sync.c) and lib/monitor.rb
# layers MonitorMixin over it.
Gem::Specification.new do |s|
  s.name = "monitor"
  s.version = "0.1.0"
  s.summary = "MonitorMixin over the native reentrant lock."
  s.require_paths = ["lib"]
end
