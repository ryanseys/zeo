class Greeter
  def method_missing(name)
    puts :missing
  end
end

Greeter.new.send(:nonexistent)
