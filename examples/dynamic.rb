class Greeter
  define_method(:hello) do
    puts :hi
  end
end

name = :hello
Greeter.new.send(name)
