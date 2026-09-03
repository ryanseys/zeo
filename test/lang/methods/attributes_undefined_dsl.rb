# The spinel-era compile-time `attributes` DSL does not exist: CRuby (and
# zeo) raise NoMethodError when the class body executes. Rescued inside
# the body so the output is path-portable.
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
