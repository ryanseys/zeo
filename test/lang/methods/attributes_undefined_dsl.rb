# There is no `attributes` class-body DSL, so the call raises NoMethodError
# when the class body runs. The rescue is inside the body, which keeps the
# output free of a path.
class BadAttributes
  begin
    attributes "name"
  rescue NoMethodError => e
    puts "rescued: #{e.message}"
  end
end
puts "after"
__END__
rescued: undefined method 'attributes' for class BadAttributes
after
