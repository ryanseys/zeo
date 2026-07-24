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
