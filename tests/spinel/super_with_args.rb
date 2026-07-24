# super (bare or with explicit args) must propagate Child param
# types to Parent params; otherwise spinel infers Parent's params
# as default int and emits a wrong-type call.
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
