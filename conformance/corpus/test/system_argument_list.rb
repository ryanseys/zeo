# Multi-argument system calls execute the command directly with its argv list.

system("ruby", "-e", "puts 'stmt_ok'")

ok = system("ruby", "-e", "exit 0")
puts ok
puts($? == 0)

ok = system("ruby", "-e", "exit 1")
puts ok
puts($? == 0)

ok = system("ruby", "-e", "exit(ARGV[0] == 'ok' ? 0 : 1)", "ok")
puts ok
puts($? == 0)
