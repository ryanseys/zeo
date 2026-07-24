a = { "O" => "Off-topic", "" => "Cancel" }
b = a.merge({ "Q" => "Low Quality" })
puts b.length
p b["Q"]
p a.length
c = a.merge({ "O" => "Override" })
p c["O"]
p c.length
