# Three separate objects (not sibling methods on one class) --
# implicit-self calls to a sibling method aren't supported yet
# (a separate, pre-existing, documented gap), so each
# level calls the next via an explicit receiver instead. Still a
# genuine 3-frame unwind, crossing object boundaries too.

class Level3
  def run
    raise "deep failure"
  end
end
class Level2
  def run
    Level3.new.run
  end
end
class Level1
  def run
    Level2.new.run
  end
end
begin
  Level1.new.run
rescue => e
  puts "caught from deep: #{e.send(:message)}"
end
__END__
caught from deep: deep failure
