# A Ruby octal escape `\033` is the ESC byte, not a group-0 backreference;
# it must compile and match rather than raising "Invalid back reference".

s = "a\e[31mred\e[0mb"
puts s.gsub(/\033\[[0-9;]*[A-Za-z]/, "")
puts ("x\033y" =~ /\033/).inspect
__END__
aredb
1
