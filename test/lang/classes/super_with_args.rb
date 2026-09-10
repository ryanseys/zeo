# super, bare or with explicit arguments, passes the child's argument VALUES
# to the parent, so the parent's parameters take their types from what the
# child actually hands over.
class Parent
  def greet(name)
    puts "Parent: #{name}"
  end
end

class Child < Parent
  def greet(name)
    super
    puts "Child: #{name}"
  end
end

class Echo < Parent
  def greet(name)
    super(name)
    puts "Echo done"
  end
end

Child.new.greet("Alice")
Echo.new.greet("Bob")
__END__
Parent: Alice
Child: Alice
Parent: Bob
Echo done
