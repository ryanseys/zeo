# Each eval of a string parses it again, so a regexp literal in it is a new
# object every time, and a `/o` literal builds again.
p 2.times.map { eval('/a/') }.map(&:object_id).uniq.size
p 2.times.map { |i| eval('/#{i}/o') }
__END__
2
[/0/, /1/]
