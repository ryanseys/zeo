begin
  puts "body"
rescue
  puts "rescued"
else
  puts "else ran"
ensure
  puts "ensure ran"
end
__END__
body
else ran
ensure ran
