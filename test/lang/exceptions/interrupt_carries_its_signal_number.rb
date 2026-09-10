# Interrupt#signo is SIGINT's number, whether raised as a class or with a message.
begin
  raise Interrupt
rescue Interrupt => e
  p e.signo
end
begin
  raise Interrupt, "boom"
rescue Interrupt => e
  p e.signo
  p e.message
end
p Interrupt.new.signo
__END__
2
2
"boom"
2
