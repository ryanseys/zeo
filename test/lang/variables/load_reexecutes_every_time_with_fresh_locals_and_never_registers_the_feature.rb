# Three executions: two `load`s plus a `require` -- load never adds to
# the feature table in real Ruby, so the require still fires. `ticks`
# restarts at 0 each execution (fresh local scope per load), which the
# per-splice-instance gensym reproduces.

Dir.chdir(File.expand_path("load_reexecutes_every_time_with_fresh_locals_and_never_registers_the_feature", __dir__))
require_relative "load_reexecutes_every_time_with_fresh_locals_and_never_registers_the_feature/main"
__END__
tick! total=1
tick! total=2
tick! total=3
end total=3
