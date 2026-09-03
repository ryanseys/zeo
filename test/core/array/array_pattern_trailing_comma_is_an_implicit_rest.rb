case [0, 1, 2, 3]
in [0, 1, ] then puts "matched"
else puts "no"
end
case [5]
in [0, 1, ] then puts "wrong"
else puts "too short"
end
__END__
matched
too short
