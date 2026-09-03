# The fiber suspends MID-rescue; the resumer then handles (and
# finishes handling) its own exception; on re-entry the fiber's bare
# re-raise must still see the FIBER's exception -- the save/restore
# swap around every switch, exercised in both directions.

g = Fiber.new do
  begin
    raise "fiber's own"
  rescue RuntimeError
    Fiber.yield :suspended_mid_rescue
    begin
      raise
    rescue RuntimeError => again
      again.send(:message)
    end
  end
end
puts g.resume
begin
  raise "resumer noise"
rescue RuntimeError
  x = 1
end
puts g.resume
__END__
suspended_mid_rescue
fiber's own
