a = { "O" => "Off-topic", "" => "Cancel" }
b = a.merge({ "Q" => "Low Quality" })
puts b.length
p b["Q"]
p a.length
c = a.merge({ "O" => "Override" })
p c["O"]
p c.length
__END__
3
"Low Quality"
2
"Override"
2
