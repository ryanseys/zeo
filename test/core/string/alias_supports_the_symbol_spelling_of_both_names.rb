class Greeter
  def hello
    "hi"
  end
  alias :bonjour :hello
end
puts Greeter.new.bonjour
__END__
hi
