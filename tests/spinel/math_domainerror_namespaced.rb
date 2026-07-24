# raise/rescue a builtin namespaced exception under its qualified name.
begin
  raise Math::DomainError, "boom"
rescue Math::DomainError => e
  puts "#{e.class}: #{e.message}"
end
begin
  Math.sqrt(-1)
rescue Math::DomainError => e
  puts e.class
end
