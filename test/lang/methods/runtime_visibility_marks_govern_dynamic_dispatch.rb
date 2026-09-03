# Runtime `Module#private`/`public`/`protected` with name arguments
# (`class_eval { private :m }`): marks land in the overlay and govern the
# DYNAMIC paths -- `respond_to?`, `public_send`, plain `send` -- with the
# return shapes, NameError timing, and an alias inheriting its source's
# runtime-marked visibility all oracle-verified verbatim. (Static call
# sites with literal names resolve visibility at compile time and don't
# see runtime marks -- the documented AOT boundary -- so every probe here
# uses a computed name.)

class C
  def m
    1
  end
  def m2
    2
  end
end
p C.class_eval { private :m }
p C.class_eval { private :m, :m2 }
sym = :m
p C.new.respond_to?(sym)
begin
  C.new.public_send(sym)
rescue NoMethodError => e
  puts e.message
end
p C.new.send(sym)
p C.class_eval { public "m" }
p C.new.respond_to?(sym)
p C.new.public_send(sym)
class D
  def p1
    3
  end
end
D.class_eval { protected :p1 }
psym = :p1
begin
  D.new.public_send(psym)
rescue NoMethodError => e
  puts e.message
end
begin
  C.class_eval { private :nope }
rescue NameError => e
  puts e.message
end
class Al
  def hidden
    4
  end
end
Al.class_eval { private :hidden }
Al.class_eval { [[:hidden2, :hidden]].each { |a, b| alias_method(a, b) } }
h2 = :hidden2
begin
  Al.new.public_send(h2)
rescue NoMethodError => e
  puts e.message
end
p Al.new.send(h2)
puts "done"
__END__
:m
[:m, :m2]
false
private method 'm' called for an instance of C
1
"m"
true
1
protected method 'p1' called for an instance of D
undefined method 'nope' for class 'C'
private method 'hidden2' called for an instance of Al
4
done
