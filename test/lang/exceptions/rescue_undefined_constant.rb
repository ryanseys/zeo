# A `rescue` clause naming an undefined constant compiles cleanly: CRuby only
# evaluates the clause's class while matching an actually-raised exception.

# Never fires -> the undefined name is never evaluated.
begin
  1 + 1
rescue NeverDefined
  puts "unreachable"
end
puts "no exception: ok"

# Fires -> evaluating the undefined constant is a runtime NameError, which
# replaces the original exception.
begin
  begin
    raise "boom"
  rescue NeverDefined
    puts "unreachable"
  end
rescue => e
  puts "#{e.class}: #{e.message}"
end
__END__
no exception: ok
NameError: uninitialized constant NeverDefined
