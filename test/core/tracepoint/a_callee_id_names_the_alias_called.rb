# `TracePoint#callee_id` is the name the traced call used: a run-time
# alias's for a call through it, where `method_id` stays the name the method
# was defined under. Both `:call` and `:return` report it.
class R
  def base
    1
  end
end
R.send(:alias_method, :late, :base)
seen = []
tp = TracePoint.new(:call, :return) { |t| seen << [t.event, t.method_id, t.callee_id] }
tp.enable
R.new.late
R.new.base
tp.disable
seen.select { |_, id, _| id == :base }.each { p _1 }
__END__
[:call, :base, :late]
[:return, :base, :late]
[:call, :base, :base]
[:return, :base, :base]
