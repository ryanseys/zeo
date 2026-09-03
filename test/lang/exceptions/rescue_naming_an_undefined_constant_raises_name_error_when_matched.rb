# When the body DOES raise, evaluating the undefined rescue constant is a
# runtime NameError (which replaces the original exception) -- exactly
# CRuby's behavior.

begin
  begin
    raise "boom"
  rescue NeverDefined
    puts "caught"
  end
rescue => e
  puts "#{e.class}: #{e.message}"
end
__END__
NameError: uninitialized constant NeverDefined
