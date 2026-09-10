# `if m && m[1].to_i < 999` over strings that match and one that does not.
["12-a", "xx", "999-z"].each do |s|
  m = s.match(/^(\d+)-/)
  if m && m[1].to_i < 999
    puts "yes #{m[1]}"
  else
    puts "no"
  end
end
__END__
yes 12
no
no
