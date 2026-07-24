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
