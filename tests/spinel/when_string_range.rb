# `when ("a".."e")`: the parentheses hid the range from the literal check, so
# the arm declined and the whole when folded to false. A String range held in a
# variable had no arm at all.
case "c" when ("a".."e") then p :in else p :out end
case "z" when ("a".."e") then p :in else p :out end
case "c" when "a".."e" then p :in2 else p :out2 end
p(("a".."e") === "c")
r = ("a".."e")
case "c" when r then p :in3 else p :out3 end
case "z" when r then p :in3 else p :out3 end
case 3 when (1..5) then p :n_in else p :n_out end
case 9 when (1..5) then p :n_in else p :n_out end
case "e" when ("a"..."e") then p :x_in else p :x_out end
