s = "a\e[31mred\e[0mb"
puts s.gsub(/\033\[[0-9;]*[A-Za-z]/, "")

t = "x\033y"
puts (t =~ /\033/).inspect
puts t.match?(/\033/)

u = "p\x1bq"
puts (u =~ /\x1b/).inspect
