# `require X if RUBY_ENGINE == "jruby"` names another engine's code; a
# whole-program AOT compiler eager-splices every literal require it sees,
# but this one's guard folds statically false on CRuby-targeting zeo, so
# the target must be PRUNED, not spliced (splicing would fail to resolve
# it). The program compiles and the guarded require is simply dead.

require "no_such_engine_lib_xyz" if RUBY_ENGINE == "jruby"
puts "ok"
__END__
ok
