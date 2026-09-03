# `puts` on `nil` prints an empty line -- real Ruby's own behavior, and
# avoids `Object#inspect` (a separate, unrelated, unimplemented method).

class Collector
  def each_one(a)
    yield a
  end
end
Collector.new.each_one(1) { |a, b| puts a; puts b }
__END__
1

