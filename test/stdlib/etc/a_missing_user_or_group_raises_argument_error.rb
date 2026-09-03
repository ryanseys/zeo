require "etc"
begin
  Etc.getpwnam("no_such_user_zqxjkw")
rescue ArgumentError => e
  puts "user: #{e.class}"
end
begin
  Etc.getgrnam("no_such_group_zqxjkw")
rescue ArgumentError => e
  puts "group: #{e.class}"
end
__END__
user: ArgumentError
group: ArgumentError
