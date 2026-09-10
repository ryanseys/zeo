# `raise SignalException, "INT"` gives an exception whose signo and message name that signal.
begin
  raise SignalException, "INT"
rescue SignalException => e
  p e.signo
  p e.message
end
begin
  raise SignalException, "TERM"
rescue SignalException => e
  p e.signo
end
begin
  raise SignalException, "SIGINT"
rescue SignalException => e
  p e.signo
end
__END__
2
"SIGINT"
15
2
