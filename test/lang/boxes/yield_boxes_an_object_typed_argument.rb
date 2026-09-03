# (`Builder.new { }` can't be used here: `.new` doesn't forward its
# block to `initialize` yet -- a separate, pre-existing gap.)

class Builder
  def run
    yield self
  end
  def ping = "pong"
end
Builder.new.run { |b| puts b.send(:ping) }
__END__
pong
