# CRuby's behavior: the error belongs to the CALL, not the program.

module M
  def self.run!(a, b, c, d)
    "#{a}#{b}#{c}#{d}"
  end
end
begin
  M.run!(1, 2, false)
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
puts M.run!(1, 2, 3, 4)
def never_called
  M.run!(1)
end
puts "done"
__END__
ArgumentError: wrong number of arguments (given 3, expected 4)
1234
done
