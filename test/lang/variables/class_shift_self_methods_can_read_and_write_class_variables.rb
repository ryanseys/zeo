class Counter
  class << self
    def reset
      @@total = 0
    end
    def bump
      @@total += 1
    end
    def total
      @@total
    end
  end
end
Counter.reset
Counter.bump
Counter.bump
puts Counter.total
__END__
2
