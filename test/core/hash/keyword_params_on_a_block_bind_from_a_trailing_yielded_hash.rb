class Collector
  def emit
    yield x: 1, y: 2
  end
end
Collector.new.emit { |x:, y: 10| puts x + y }
__END__
3
