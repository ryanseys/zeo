# Exercises the class-method body lowering's own `Signal::Return`
# catch (added specifically for `Begin` nodes inside a class method --
# class methods can't contain an escaping block at all, so `Begin` was
# the only possible trigger there).

class Factory
  def self.build(fail_it)
    begin
      raise "nope" if fail_it
      "built"
    rescue
      return "fallback"
    ensure
      puts "factory ensure"
    end
  end
end
puts Factory.build(false)
puts Factory.build(true)
__END__
factory ensure
built
factory ensure
fallback
